mod element;
mod style;
mod update;

use crate::VERSION;
use ajour_core::{
    addon::{Addon, ReleaseChannel},
    config::{load_config, Config, Flavor},
    error::ClientError,
    fs::PersistentData,
    parse::FingerprintCollection,
    theme::{load_user_themes, Theme},
    utility::needs_update,
    Result,
};
use async_std::sync::{Arc, Mutex};
use chrono::NaiveDateTime;
use iced::{
    button, pick_list, scrollable, Application, Column, Command, Container, Element, Length,
    Settings, Space, Subscription,
};
use isahc::{
    config::{Configurable, RedirectPolicy},
    HttpClient,
};
use std::collections::HashMap;
use std::path::PathBuf;
use widgets::header;

use image::ImageFormat;
static WINDOW_ICON: &[u8] = include_bytes!("../../resources/windows/ajour.ico");

#[derive(Debug)]
pub enum AjourState {
    Error(ClientError),
    Idle,
    Loading,
    Welcome,
}

#[derive(Debug, Clone)]
pub enum Interaction {
    Delete(String),
    Expand(String),
    Ignore(String),
    OpenDirectory(DirectoryType),
    OpenLink(String),
    Refresh,
    Settings,
    Unignore(String),
    Update(String),
    UpdateAll,
    SortColumn(SortKey),
    FlavorSelected(Flavor),
    ResizeColumn(header::ResizeEvent),
    ScaleUp,
    ScaleDown,
    Backup,
}

#[derive(Debug)]
pub enum Message {
    DownloadedAddon((String, Result<()>)),
    Error(ClientError),
    Interaction(Interaction),
    NeedsUpdate(Result<Option<String>>),
    None(()),
    Parse(Result<Config>),
    ParsedAddons((Flavor, Result<Vec<Addon>>)),
    UpdateFingerprint((String, Result<()>)),
    ThemeSelected(String),
    ReleaseChannelSelected(ReleaseChannel),
    ThemesLoaded(Vec<Theme>),
    UnpackedAddon((String, Result<()>)),
    UpdateWowDirectory(Option<PathBuf>),
    UpdateBackupDirectory(Option<PathBuf>),
    RuntimeEvent(iced_native::Event),
    LatestBackup(Option<NaiveDateTime>),
    BackupFinished(Result<NaiveDateTime>),
}

pub struct Ajour {
    addons: HashMap<Flavor, Vec<Addon>>,
    addons_scrollable_state: scrollable::State,
    config: Config,
    directory_btn_state: button::State,
    expanded_addon: Option<Addon>,
    ignored_addons: HashMap<Flavor, Vec<(Addon, button::State)>>,
    ignored_addons_scrollable_state: scrollable::State,
    is_showing_settings: bool,
    needs_update: Option<String>,
    new_release_button_state: button::State,
    refresh_btn_state: button::State,
    settings_btn_state: button::State,
    shared_client: Arc<HttpClient>,
    state: AjourState,
    update_all_btn_state: button::State,
    header_state: HeaderState,
    theme_state: ThemeState,
    fingerprint_collection: Arc<Mutex<Option<FingerprintCollection>>>,
    retail_btn_state: button::State,
    classic_btn_state: button::State,
    scale_state: ScaleState,
    backup_state: BackupState,
}

impl Default for Ajour {
    fn default() -> Self {
        Self {
            addons: HashMap::new(),
            addons_scrollable_state: Default::default(),
            config: Config::default(),
            directory_btn_state: Default::default(),
            expanded_addon: None,
            ignored_addons: Default::default(),
            ignored_addons_scrollable_state: Default::default(),
            is_showing_settings: false,
            needs_update: None,
            new_release_button_state: Default::default(),
            refresh_btn_state: Default::default(),
            settings_btn_state: Default::default(),
            shared_client: Arc::new(
                HttpClient::builder()
                    .redirect_policy(RedirectPolicy::Follow)
                    .max_connections_per_host(6)
                    .build()
                    .unwrap(),
            ),
            state: AjourState::Loading,
            update_all_btn_state: Default::default(),
            header_state: Default::default(),
            theme_state: Default::default(),
            fingerprint_collection: Arc::new(Mutex::new(None)),
            retail_btn_state: Default::default(),
            classic_btn_state: Default::default(),
            scale_state: Default::default(),
            backup_state: Default::default(),
        }
    }
}

impl Application for Ajour {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        let init_commands = vec![
            Command::perform(load_config(), Message::Parse),
            Command::perform(needs_update(VERSION), Message::NeedsUpdate),
            Command::perform(load_user_themes(), Message::ThemesLoaded),
        ];

        (Ajour::default(), Command::batch(init_commands))
    }

    fn title(&self) -> String {
        String::from("Ajour")
    }

    fn scale_factor(&self) -> f64 {
        self.scale_state.scale
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        iced_native::subscription::events().map(Message::RuntimeEvent)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match update::handle_message(self, message) {
            Ok(x) => x,
            Err(e) => Command::perform(async { e }, Message::Error),
        }
    }

    fn view(&mut self) -> Element<Message> {
        // Clone config to be used.
        // FIXME: This could be done prettier.
        let cloned_config = self.config.clone();

        // Get color palette of chosen theme.
        let color_palette = self
            .theme_state
            .themes
            .iter()
            .find(|(name, _)| name == &self.theme_state.current_theme_name)
            .as_ref()
            .unwrap_or(&&("Dark".to_string(), Theme::dark()))
            .1
            .palette;

        // Get addons for current flavor.
        let flavor = self.config.wow.flavor;
        let addons = self.addons.entry(flavor).or_default();

        // Get the ignored addons ids.
        let ignored_strings = self.config.addons.ignored.get(&flavor).cloned();

        // Get ignored addons for flavor.
        let ignored_addons = self.ignored_addons.entry(flavor).or_default();

        // Check if we have any addons.
        let has_addons = !&addons.is_empty();

        // Menu container at the top of the applications.
        // This has all global buttons, such as Settings, Update All, etc.
        let menu_container = element::menu_container(
            color_palette,
            &mut self.update_all_btn_state,
            &mut self.refresh_btn_state,
            &mut self.retail_btn_state,
            &mut self.classic_btn_state,
            &mut self.settings_btn_state,
            &self.state,
            addons,
            &mut self.config,
            self.needs_update.as_deref(),
            &mut self.new_release_button_state,
        );

        let title_width = self.header_state.title.width;
        let local_width = self.header_state.local_version.width;
        let remote_width = self.header_state.remote_version.width;
        let status_width = self.header_state.status.width;

        // Addon row titles is a row of titles above the addon scrollable.
        // This is to add titles above each section of the addon row, to let
        // the user easily identify what the value is.
        let addon_row_titles =
            element::addon_row_titles(color_palette, addons, &mut self.header_state);

        // A scrollable list containing rows.
        // Each row holds data about a single addon.
        let mut addons_scrollable =
            element::addon_scrollable(color_palette, &mut self.addons_scrollable_state);

        // Loops though the addons.
        for addon in &mut addons
            .iter_mut()
            .filter(|a| !a.is_ignored(ignored_strings.as_ref()))
        {
            // Checks if the current addon is expanded.
            let is_addon_expanded = match &self.expanded_addon {
                Some(expanded_addon) => addon.id == expanded_addon.id,
                None => false,
            };

            // A container cell which has all data about the current addon.
            // If the addon is expanded, then this is also included in this container.
            let addon_data_cell = element::addon_data_cell(
                color_palette,
                addon,
                is_addon_expanded,
                title_width,
                local_width,
                remote_width,
                status_width,
            );

            // Adds the addon data cell to the scrollable.
            addons_scrollable = addons_scrollable.push(addon_data_cell);
        }

        // Bottom space below the scrollable.
        let bottom_space = Space::new(Length::FillPortion(1), Length::Units(10));

        // This column gathers all the other elements together.
        let mut content = Column::new().push(menu_container);

        // This ensure we only draw settings, when we need to.
        if self.is_showing_settings {
            // Settings container, containing all data releated to settings.
            let settings_container = element::settings_container(
                color_palette,
                &mut self.directory_btn_state,
                &mut self.ignored_addons_scrollable_state,
                ignored_addons,
                &cloned_config,
                &mut self.theme_state,
                &mut self.scale_state,
                &mut self.backup_state,
            );

            // Space below settings.
            let space = Space::new(Length::Fill, Length::Units(10));

            // Adds the settings container.
            content = content.push(settings_container).push(space);
        }

        // Adds the rest of the elements to the content column.
        if has_addons {
            content = content
                .push(addon_row_titles)
                .push(addons_scrollable)
                .push(bottom_space)
        }

        // Status messages.
        let container: Option<Container<Message>> = match self.state {
            AjourState::Welcome => Some(element::status_container(
                color_palette,
                "Welcome to Ajour!",
                "To get started, go to Settings and select your World of Warcraft directory.",
            )),
            AjourState::Idle => {
                if !has_addons {
                    Some(element::status_container(
                        color_palette,
                        "Woops!",
                        &format!("You have no {} addons.", flavor.to_string().to_lowercase()),
                    ))
                } else {
                    None
                }
            }
            AjourState::Loading => Some(element::status_container(
                color_palette,
                "Loading..",
                "Currently parsing addons.",
            )),
            _ => None,
        };

        if let Some(c) = container {
            content = content.push(c);
        };

        // Small padding to make UI fit better.
        content = content.padding(3);

        // Finally wraps everything in a container.
        Container::new(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::Content(color_palette))
            .into()
    }
}

/// Starts the GUI.
/// This function does not return.
pub fn run() {
    let config: Config = Config::load_or_default().expect("loading config on application startup");

    let mut settings = Settings::default();
    settings.window.size = config.window_size.unwrap_or((900, 620));
    // Enforce the usage of dedicated gpu if available.
    settings.antialiasing = true;

    // Sets the Window icon.
    let image = image::load_from_memory_with_format(WINDOW_ICON, ImageFormat::Ico)
        .expect("loading icon")
        .to_rgba();
    let (width, height) = image.dimensions();
    let icon = iced::window::Icon::from_rgba(image.into_raw(), width, height);
    settings.window.icon = Some(icon.unwrap());

    // Runs the GUI.
    Ajour::run(settings);
}

#[derive(Debug, Clone, Copy)]
pub enum DirectoryType {
    Wow,
    Backup,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
pub enum SortKey {
    Title,
    LocalVersion,
    RemoteVersion,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    fn toggle(self) -> SortDirection {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }
}

pub struct HeaderState {
    state: header::State,
    previous_sort_key: Option<SortKey>,
    previous_sort_direction: Option<SortDirection>,
    title: ColumnState,
    local_version: ColumnState,
    remote_version: ColumnState,
    status: ColumnState,
}

impl Default for HeaderState {
    fn default() -> Self {
        Self {
            state: Default::default(),
            previous_sort_key: None,
            previous_sort_direction: None,
            title: ColumnState {
                btn_state: Default::default(),
                width: Length::Fill,
            },
            local_version: ColumnState {
                btn_state: Default::default(),
                width: Length::Units(150),
            },
            remote_version: ColumnState {
                btn_state: Default::default(),
                width: Length::Units(150),
            },
            status: ColumnState {
                btn_state: Default::default(),
                width: Length::Units(85),
            },
        }
    }
}

pub struct ColumnState {
    btn_state: button::State,
    width: Length,
}

pub struct ThemeState {
    themes: Vec<(String, Theme)>,
    current_theme_name: String,
    pick_list_state: pick_list::State<String>,
}

impl Default for ThemeState {
    fn default() -> Self {
        let mut themes = vec![];
        themes.push(("Dark".to_string(), Theme::dark()));
        themes.push(("Light".to_string(), Theme::light()));

        ThemeState {
            themes,
            current_theme_name: "Dark".to_string(),
            pick_list_state: Default::default(),
        }
    }
}

pub struct ScaleState {
    scale: f64,
    up_btn_state: button::State,
    down_btn_state: button::State,
}

impl Default for ScaleState {
    fn default() -> Self {
        ScaleState {
            scale: 1.0,
            up_btn_state: Default::default(),
            down_btn_state: Default::default(),
        }
    }
}

#[derive(Default)]
pub struct BackupState {
    backing_up: bool,
    last_backup: Option<NaiveDateTime>,
    directory_btn_state: button::State,
    backup_now_btn_state: button::State,
}
