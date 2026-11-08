//! The puron-applet program. The event loop of `app` runs on its own
//! thread with the engine, the config and the program places. libcosmic's
//! applet runs on the main thread and draws the panel button (`panel`)
//! and the popup (`popup`) from the state the loop sends after each
//! event. Popup clicks go to the loop as UI events. When libcosmic's run
//! returns (the panel removed the applet or closed its connection), an
//! `Event::Stop` goes to the loop, which exits after the engine has shut
//! down.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use cosmic::app::{Core, Task};
use cosmic::iced::futures::channel::mpsc::{unbounded, UnboundedReceiver};
use cosmic::iced::futures::stream::{self, BoxStream, StreamExt};
use cosmic::iced::platform_specific::shell::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::window::Id;
use cosmic::iced::Subscription;
use cosmic::widget::button;
use cosmic::Element;
use puron_applet::app::{self, App, Event, Output, Programs, Source};
use puron_applet::{panel, popup};
use puron_config::Config;
use puron_core::menu::Menu;
use puron_core::panel::Panel;
use puron_engine::clock::SystemClock;
use puron_engine::engine::Engine;

/// The state the UI draws, as `app` holds it after one event.
#[derive(Clone, Debug)]
struct View {
    menu: Option<Menu>,
    panel: Panel,
    pinned: Vec<String>,
}

#[derive(Clone, Debug)]
enum Msg {
    View(Box<View>),
    Toggle,
    Clicked(popup::Clicked),
    PopupClosed(Id),
}

/// The receiving end of the loop's views, taken once by the
/// subscription.
static VIEWS: Mutex<Option<UnboundedReceiver<View>>> = Mutex::new(None);

fn views() -> BoxStream<'static, Msg> {
    match VIEWS.lock().unwrap().take() {
        Some(rx) => rx.map(|v| Msg::View(Box::new(v))).boxed(),
        None => stream::pending().boxed(),
    }
}

struct Applet {
    core: Core,
    tx: Sender<(Source, Event)>,
    view: View,
    popup_id: Option<Id>,
}

impl Applet {
    fn popup_plan(&self) -> Option<popup::Plan> {
        let menu = self.view.menu.as_ref()?;
        let size = panel::icon_size(&self.core.applet);
        Some(popup::plan(menu, &self.view.pinned, size))
    }

    fn send(&self, e: popup::Event) {
        let event = match e {
            popup::Event::PinClick(id) => Event::PinClick(id),
            popup::Event::HomepageLink => Event::HomepageLink,
            popup::Event::SettingsLink => Event::SettingsLink,
        };
        let _ = self.tx.send((Source::Ui, event));
    }

    fn close(&mut self) -> Task<Msg> {
        match self.popup_id.take() {
            Some(id) => destroy_popup(id),
            None => Task::none(),
        }
    }
}

impl cosmic::Application for Applet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = Sender<(Source, Event)>;
    type Message = Msg;
    /// Kept apart from the config ID, as in the settings window:
    /// libcosmic keeps its own state under the App ID, never in Puron's
    /// entries.
    const APP_ID: &'static str = "io.github.elalonde.Puron.Applet";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, tx: Self::Flags) -> (Self, Task<Msg>) {
        let applet = Applet {
            core,
            tx,
            view: View {
                menu: None,
                panel: Panel {
                    items: Vec::new(),
                    padding: 0,
                    state: Default::default(),
                },
                pinned: Vec::new(),
            },
            popup_id: None,
        };
        (applet, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Msg> {
        Some(Msg::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::run(views)
    }

    fn update(&mut self, message: Msg) -> Task<Msg> {
        match message {
            Msg::View(v) => {
                self.view = *v;
            }
            Msg::Toggle => {
                if self.popup_id.is_some() {
                    return self.close();
                }
                let Some(main) = self.core.main_window_id() else {
                    return Task::none();
                };
                let id = Id::unique();
                self.popup_id = Some(id);
                let mut settings = self
                    .core
                    .applet
                    .get_popup_settings(main, id, None, None, None);
                settings.positioner.size_limits =
                    popup::limits(popup::MAX_WIDTH).min_width(popup::WIDTH);
                return get_popup(settings);
            }
            Msg::Clicked(clicked) => {
                let Some(plan) = self.popup_plan() else {
                    return Task::none();
                };
                let reply = popup::clicked(&plan, clicked);
                if let Some(e) = reply.event {
                    self.send(e);
                }
                if reply.close {
                    return self.close();
                }
            }
            Msg::PopupClosed(id) => {
                if self.popup_id == Some(id) {
                    self.popup_id = None;
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let ctx = &self.core.applet;
        let plan = panel::plan(
            &self.view.panel,
            panel::Orientation::of(ctx),
            panel::icon_size(ctx),
        );
        let (major, minor) = ctx.suggested_padding(true);
        let (h, v) = if ctx.is_horizontal() {
            (major, minor)
        } else {
            (minor, major)
        };
        let b = button::custom(panel::view::<Msg>(&plan))
            .padding([f32::from(v), f32::from(h)])
            .class(cosmic::theme::Button::AppletIcon)
            .on_press(Msg::Toggle);
        ctx.autosize_window(b).into()
    }

    fn view_window(&self, id: Id) -> Element<'_, Msg> {
        match (self.popup_id == Some(id), self.popup_plan()) {
            (true, Some(plan)) => {
                let content = popup::view(&plan).map(Msg::Clicked);
                let width = self
                    .view
                    .menu
                    .as_ref()
                    .map_or(popup::WIDTH, |m| popup::width(m, plan.icon_size));
                self.core
                    .applet
                    .popup_container(content)
                    .limits(popup::limits(width))
                    .into()
            }
            _ => cosmic::widget::Space::new().into(),
        }
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

fn main() {
    puron_applet::start();

    let (tx, rx) = mpsc::channel();
    let (vtx, vrx) = unbounded();
    *VIEWS.lock().unwrap() = Some(vrx);

    let engine = Arc::new(Engine::new(SystemClock::new()));
    let config = Config::new(puron_config::user_root().unwrap_or_default());
    let state = App::new(engine, config, Programs::production());
    let loop_tx = tx.clone();
    let event_loop = std::thread::spawn(move || {
        app::run(state, loop_tx, rx, move |_, a: &App, _: &[Output]| {
            let _ = vtx.unbounded_send(View {
                menu: a.menu().cloned(),
                panel: a.panel().clone(),
                pinned: a.pinned().to_vec(),
            });
        })
    });

    // A toolkit that cannot start panics (no display). That ends the
    // event loop too, so the engine is still shut down.
    let ui_tx = tx.clone();
    let _ = std::panic::catch_unwind(move || cosmic::applet::run::<Applet>(ui_tx));
    // The toolkit's event loop ended: send `Event::Stop`. Our loop exits the
    // process with status 0 once the engine shutdown has returned.
    let _ = tx.send((Source::Stop, Event::Stop));
    // The loop returns only by a panic (the profile unwinds). No
    // shutdown was sent, so the process ends here with a failure status
    // rather than hang with the engine's threads still alive.
    let code = if event_loop.join().is_err() { 1 } else { 0 };
    std::process::exit(code);
}
