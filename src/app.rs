use config::{AppTheme, TimeFmt, CONFIG_VERSION};
use cosmic::cosmic_config::Update;
use cosmic::cosmic_theme::ThemeMode;
use cosmic::iced::keyboard::{Key, Modifiers};
use cosmic::widget::menu::action::MenuAction;
use cosmic::widget::menu::key_bind::KeyBind;
use cosmic::{
    app::{Command, Core},
    cosmic_config::{self, CosmicConfigEntry},
    cosmic_theme, executor,
    iced::{event, keyboard::Event as KeyEvent, window, Alignment, Event, Length, Subscription},
    theme, widget,
    widget::{column, container, nav_bar, scrollable},
    ApplicationExt, Apply, Element,
};
use std::any::TypeId;
use std::collections::{HashMap, VecDeque};

pub mod config;
pub mod icon_cache;
pub mod key_bind;
pub mod localize;
pub mod menu;
pub mod settings;

use crate::app::config::{Units, WeatherConfig};
use crate::app::icon_cache::icon_cache_get;
use crate::app::key_bind::key_binds;
use crate::fl;
use crate::model::location::Location;
use crate::model::weather::WeatherData;

#[derive(Clone, Debug)]
pub enum Message {
    ChangeCity,
    Quit,
    SystemThemeModeChange,
    ToggleContextPage(ContextPage),
    LaunchUrl(String),
    Key(Modifiers, Key),
    Modifiers(Modifiers),
    Config(WeatherConfig),
    Units(Units),
    TimeFmt(TimeFmt),
    AppTheme(AppTheme),
    DialogComplete(String),
    DialogCancel,
    DialogUpdate(DialogPage),
    SetLocation(Location),
    SetWeatherData(WeatherData),
    Error(String),
}

#[derive(Clone, Debug)]
pub struct Flags {
    pub config_handler: Option<cosmic_config::Config>,
    pub config: WeatherConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextPage {
    About,
    Settings,
}

impl ContextPage {
    fn title(&self) -> String {
        match self {
            Self::About => fl!("about"),
            Self::Settings => fl!("settings"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialogPage {
    Change(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    About,
    Settings,
    ChangeCity,
    Quit,
}

impl MenuAction for Action {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            Action::About => Message::ToggleContextPage(ContextPage::About),
            Action::Settings => Message::ToggleContextPage(ContextPage::Settings),
            Action::ChangeCity => Message::ChangeCity,
            Action::Quit => Message::Quit,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavPage {
    HourlyView,
    DailyView,
    Details,
}

impl NavPage {
    fn all() -> &'static [Self] {
        &[Self::HourlyView, Self::DailyView, Self::Details]
    }

    fn title(&self) -> String {
        match self {
            Self::HourlyView => fl!("hourly-forecast"),
            Self::DailyView => fl!("daily-forecast"),
            Self::Details => fl!("details"),
        }
    }

    fn icon(&self) -> widget::icon::Icon {
        match self {
            Self::HourlyView => icon_cache_get("view-hourly", 16),
            Self::DailyView => icon_cache_get("view-daily", 16),
            Self::Details => icon_cache_get("view-detail", 16),
        }
    }
}

pub struct App {
    core: Core,
    nav_model: nav_bar::Model,
    key_binds: HashMap<KeyBind, Action>,
    modifiers: Modifiers,
    context_page: ContextPage,
    config_handler: Option<cosmic_config::Config>,
    pub config: WeatherConfig,
    pub weather_data: WeatherData,
    units: Vec<String>,
    timefmt: Vec<String>,
    app_themes: Vec<String>,
    dialog_pages: VecDeque<DialogPage>,
    dialog_page_text: widget::Id,
}

impl cosmic::Application for App {
    type Executor = executor::Default;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = "com.jwestall.Weather";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let mut nav_model = nav_bar::Model::default();
        for &nav_page in NavPage::all() {
            let id = nav_model
                .insert()
                .icon(nav_page.icon())
                .text(nav_page.title())
                .data::<NavPage>(nav_page)
                .id();
            if nav_page == NavPage::HourlyView {
                nav_model.activate(id);
            }
        }

        let mut commands = vec![];
        let app_units = vec![fl!("fahrenheit"), fl!("celsius")];
        let app_timefmt = vec![fl!("twelve-hr"), fl!("twenty-four-hr")];
        let app_themes = vec![fl!("light"), fl!("dark"), fl!("system")];

        let mut app = App {
            core,
            nav_model,
            key_binds: key_binds(),
            modifiers: Modifiers::empty(),
            context_page: ContextPage::Settings,
            config_handler: flags.config_handler,
            config: flags.config,
            weather_data: WeatherData::default(),
            units: app_units,
            timefmt: app_timefmt,
            app_themes,
            dialog_pages: VecDeque::new(),
            dialog_page_text: widget::Id::unique(),
        };

        // Default location to Denver if empty
        // TODO: Default to user location
        if app.config.location.is_none() {
            let command = Command::perform(
                Location::get_location_data(String::from("Denver")),
                |data| match data {
                    Ok(data) => {
                        let Some(data) = data.first() else {
                            return cosmic::app::Message::App(Message::Error(
                                "Could not get location data.".to_string(),
                            ));
                        };
                        cosmic::app::Message::App(Message::SetLocation(data.clone()))
                    }
                    Err(err) => cosmic::app::Message::App(Message::Error(err.to_string())),
                },
            );

            commands.push(command);
        }

        // Do not open nav bar by default
        app.core.nav_bar_set_toggled(false);

        commands.push(app.update_title());
        commands.push(app.update_weather_data());

        (app, Command::batch(commands))
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav_model)
    }

    fn context_drawer(&self) -> Option<Element<Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => self.about(),
            ContextPage::Settings => self.settings(),
        })
    }

    fn dialog(&self) -> Option<Element<Message>> {
        let dialog_page = match self.dialog_pages.front() {
            Some(some) => some,
            None => return None,
        };

        let cosmic_theme::Spacing { space_xxs, .. } = theme::active().cosmic().spacing;

        let dialog = match dialog_page {
            DialogPage::Change(city) => widget::dialog(fl!("change-city"))
                .primary_action(
                    widget::button::suggested(fl!("save"))
                        .on_press_maybe(Some(Message::DialogComplete(city.to_string()))),
                )
                .secondary_action(
                    widget::button::standard(fl!("cancel")).on_press(Message::DialogCancel),
                )
                .control(
                    widget::column::with_children(vec![widget::text_input(
                        fl!("search"),
                        city.as_str(),
                    )
                    .id(self.dialog_page_text.clone())
                    .on_input(move |city| Message::DialogUpdate(DialogPage::Change(city)))
                    .into()])
                    .spacing(space_xxs),
                ),
        };

        Some(dialog.into())
    }

    fn header_start(&self) -> Vec<Element<Self::Message>> {
        vec![menu::menu_bar(&self.key_binds)]
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Command<Message> {
        self.nav_model.activate(id);

        Command::none()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        struct ConfigSubscription;
        struct ThemeSubscription;

        let subscriptions = vec![
            event::listen_with(|event, status| match event {
                Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) => match status {
                    event::Status::Ignored => Some(Message::Key(modifiers, key)),
                    event::Status::Captured => None,
                },
                Event::Keyboard(KeyEvent::ModifiersChanged(modifiers)) => {
                    Some(Message::Modifiers(modifiers))
                }
                _ => None,
            }),
            cosmic_config::config_subscription(
                TypeId::of::<ConfigSubscription>(),
                Self::APP_ID.into(),
                CONFIG_VERSION,
            )
            .map(|update: Update<ThemeMode>| {
                if !update.errors.is_empty() {
                    log::info!(
                        "errors loading config {:?}: {:?}",
                        update.keys,
                        update.errors
                    );
                }
                Message::SystemThemeModeChange
            }),
            cosmic_config::config_subscription::<_, cosmic_theme::ThemeMode>(
                TypeId::of::<ThemeSubscription>(),
                cosmic_theme::THEME_MODE_ID.into(),
                cosmic_theme::ThemeMode::version(),
            )
            .map(|update: Update<ThemeMode>| {
                if !update.errors.is_empty() {
                    log::info!(
                        "errors loading theme mode {:?}: {:?}",
                        update.keys,
                        update.errors
                    );
                }
                Message::SystemThemeModeChange
            }),
        ];

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        let mut commands = vec![];
        match message {
            Message::ChangeCity => {
                // TODO
                self.dialog_pages
                    .push_back(DialogPage::Change(String::new()));
            }
            Message::Quit => {
                return window::close(window::Id::MAIN);
            }
            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = context_page.clone();
                    self.core.window.show_context = true;
                }
                self.set_context_title(context_page.clone().title());
            }
            Message::LaunchUrl(url) => match open::that_detached(&url) {
                Ok(()) => {}
                Err(err) => {
                    log::warn!("failed to open {:?}: {}", url, err);
                }
            },
            Message::Key(modifiers, key) => {
                for (key_bind, action) in self.key_binds.iter() {
                    if key_bind.matches(modifiers, &key) {
                        return self.update(action.message());
                    }
                }
            }
            Message::Modifiers(modifiers) => {
                self.modifiers = modifiers;
            }
            Message::Config(config) => {
                if config != self.config {
                    log::info!("Updating config");
                    self.config = config;
                }
            }
            Message::Units(units) => {
                self.config.units = units;
                commands.push(self.save_config());
            }
            Message::TimeFmt(timefmt) => {
                self.config.timefmt = timefmt;
                commands.push(self.save_config());
            }
            Message::AppTheme(theme) => {
                self.config.app_theme = theme;
                commands.push(self.save_config());
                commands.push(self.save_theme());
            }
            Message::DialogComplete(city) => {
                let command =
                    Command::perform(Location::get_location_data(city), |data| match data {
                        Ok(data) => {
                            let Some(data) = data.first() else {
                                return cosmic::app::Message::App(Message::Error(
                                    "Could not get location data.".to_string(),
                                ));
                            };
                            cosmic::app::Message::App(Message::SetLocation(data.clone()))
                        }
                        Err(err) => cosmic::app::Message::App(Message::Error(err.to_string())),
                    });

                commands.push(command);
                commands.push(self.save_config());

                self.dialog_pages.pop_front();
            }
            Message::DialogCancel => {
                self.dialog_pages.pop_front();
            }
            Message::DialogUpdate(dialog_page) => {
                self.dialog_pages[0] = dialog_page;
            }
            Message::SetLocation(location) => {
                self.config.location = Some(location.display_name.clone());
                self.config.latitude = Some(location.lat.clone());
                self.config.longitude = Some(location.lon.clone());
                commands.push(self.save_config());
                commands.push(self.update_weather_data());
            }
            Message::SetWeatherData(data) => {
                self.weather_data = data;
            }
            Message::Error(err) => eprintln!("Error: {}", err),
            Message::SystemThemeModeChange => {
                commands.push(self.save_theme());
                commands.push(self.save_config());
            }
        }

        Command::batch(commands)
    }

    fn view(&self) -> Element<Self::Message> {
        let page_view = match self.nav_model.active_data::<NavPage>() {
            Some(NavPage::HourlyView) => self.view_hourly_forecast(),
            Some(NavPage::DailyView) => self.view_daily_forecast(),
            Some(NavPage::Details) => self.view_detail_forecast(),
            None => cosmic::widget::text("Unkown page selected.").into(),
        };

        column()
            .spacing(24)
            .push(container(page_view).width(Length::Fill))
            .apply(container)
            .width(Length::Fill)
            .max_width(1000)
            .apply(container)
            .center_x()
            .width(Length::Fill)
            .apply(scrollable)
            .into()
    }
}

impl App
where
    Self: cosmic::Application,
{
    fn update_title(&mut self) -> Command<Message> {
        let window_title = fl!("cosmic-ext-weather").to_string();

        self.set_header_title(window_title.clone());
        self.set_window_title(window_title)
    }

    fn save_config(&mut self) -> Command<Message> {
        if let Some(ref config_handler) = self.config_handler {
            if let Err(err) = self.config.write_entry(config_handler) {
                log::error!("failed to save config: {}", err);
            }
        }

        Command::none()
    }

    fn save_theme(&self) -> Command<Message> {
        cosmic::app::command::set_theme(self.config.app_theme.theme())
    }

    fn update_weather_data(&self) -> Command<Message> {
        if let (Some(lat), Some(long)) = (
            self.config.latitude.as_ref(),
            self.config.longitude.as_ref(),
        ) {
            let coords = (
                lat.parse::<f64>().expect("Error parsing string to f64"),
                long.parse::<f64>().expect("Error parsing string to f64"),
            );

            return Command::perform(WeatherData::get_weather_data(coords), |data| match data {
                Ok(data) => {
                    let Some(data) = data else {
                        return cosmic::app::Message::App(Message::Error(
                            "Could not get weather data.".to_string(),
                        ));
                    };
                    cosmic::app::Message::App(Message::SetWeatherData(data.clone()))
                }
                Err(err) => cosmic::app::Message::App(Message::Error(err.to_string())),
            });
        };
        Command::none()
    }

    fn about(&self) -> Element<Message> {
        let spacing = theme::active().cosmic().spacing;
        let repository = "https://github.com/jwestall/cosmic-weather";
        let hash = env!("VERGEN_GIT_SHA");
        let short_hash: String = hash.chars().take(7).collect();
        let date = env!("VERGEN_GIT_COMMIT_DATE");
        widget::column::with_children(vec![
            widget::svg(widget::svg::Handle::from_memory(
                &include_bytes!("../res/icons/hicolor/scalable/apps/com.jwestall.Weather.svg")[..],
            ))
            .into(),
            widget::text::title3(fl!("cosmic-ext-weather")).into(),
            widget::button::link(repository)
                .on_press(Message::LaunchUrl(repository.to_string()))
                .padding(spacing.space_none)
                .into(),
            widget::button::link(fl!(
                "git-description",
                hash = short_hash.as_str(),
                date = date
            ))
            .on_press(Message::LaunchUrl(format!("{repository}/commits/{hash}")))
            .padding(spacing.space_none)
            .into(),
        ])
        .align_items(Alignment::Center)
        .spacing(spacing.space_xxs)
        .width(Length::Fill)
        .into()
    }

    fn settings(&self) -> Element<Message> {
        let selected_units = match self.config.units {
            Units::Fahrenheit => 0,
            Units::Celsius => 1,
        };

        let selected_timefmt = match self.config.timefmt {
            TimeFmt::TwelveHr => 0,
            TimeFmt::TwentyFourHr => 1,
        };

        let selected_theme = match self.config.app_theme {
            config::AppTheme::Light => 0,
            config::AppTheme::Dark => 1,
            config::AppTheme::System => 2,
        };

        widget::settings::view_column(vec![
            widget::settings::view_section(fl!("general"))
                .add(
                    widget::settings::item::builder(fl!("units")).control(widget::dropdown(
                        &self.units,
                        Some(selected_units),
                        move |index| {
                            Message::Units(match index {
                                1 => Units::Celsius,
                                _ => Units::Fahrenheit,
                            })
                        },
                    )),
                )
                .add(
                    widget::settings::item::builder(fl!("time-format")).control(widget::dropdown(
                        &self.timefmt,
                        Some(selected_timefmt),
                        move |index| {
                            Message::TimeFmt(match index {
                                1 => TimeFmt::TwentyFourHr,
                                _ => TimeFmt::TwelveHr,
                            })
                        },
                    )),
                )
                .into(),
            widget::settings::view_section(fl!("appearance"))
                .add(
                    widget::settings::item::builder(fl!("theme")).control(widget::dropdown(
                        &self.app_themes,
                        Some(selected_theme),
                        move |index| {
                            Message::AppTheme(match index {
                                0 => AppTheme::Light,
                                1 => AppTheme::Dark,
                                _ => AppTheme::System,
                            })
                        },
                    )),
                )
                .into(),
        ])
        .into()
    }
}
