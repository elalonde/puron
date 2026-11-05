//! The puron-settings program. libcosmic's single-instance start runs
//! the settings window: the state machine of `window` on `config`'s
//! per-user root, drawn by `view` from its plan. Open ends with the
//! start of the config watch; each delivery goes back to the state
//! machine.

use std::sync::Mutex;

use cosmic::app::{Core, CosmicFlags, Settings, Task};
use cosmic::iced::futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use cosmic::iced::futures::stream::{self, BoxStream, StreamExt};
use cosmic::iced::{Limits, Size, Subscription};
use cosmic::{ApplicationExt, Element};
use puron_config::{Config, Delivery, Watch};
use puron_settings::view;
use puron_settings::window::{window_settings, Event, Output, Plan, Window, APP_ID, TITLE};

/// The receiving end of the deliveries, taken once by the subscription.
static DELIVERIES: Mutex<Option<UnboundedReceiver<Delivery>>> = Mutex::new(None);

fn deliveries() -> BoxStream<'static, Event> {
    match DELIVERIES.lock().unwrap().take() {
        Some(rx) => rx.map(Event::Delivery).boxed(),
        None => stream::pending().boxed(),
    }
}

struct Flags {
    config: Config,
    tx: UnboundedSender<Delivery>,
}

impl CosmicFlags for Flags {
    type SubCommand = String;
    type Args = Vec<String>;
}

struct App {
    core: Core,
    window: Window,
    /// The plan of the state after the last event, which `view` draws.
    plan: Plan,
    config: Config,
    tx: UnboundedSender<Delivery>,
    watch: Option<Watch>,
}

impl App {
    fn handle(&mut self, event: Event) {
        for out in self.window.handle(event) {
            match out {
                Output::WatchStart => {
                    let tx = Mutex::new(self.tx.clone());
                    self.watch = self.config.watch(move |d| {
                        let _ = tx.lock().unwrap().unbounded_send(d);
                    });
                }
                Output::Store { .. } | Output::Exit => {}
            }
        }
        self.plan = self.window.plan();
    }
}

impl cosmic::Application for App {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = Flags;
    type Message = Event;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Flags) -> (Self, Task<Event>) {
        let window = Window::new(flags.config.clone());
        let mut app = App {
            core,
            plan: window.plan(),
            window,
            config: flags.config,
            tx: flags.tx,
            watch: None,
        };
        app.set_header_title(TITLE.to_string());
        app.handle(Event::Open);
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title(TITLE.to_string(), id),
            None => Task::none(),
        };
        (app, title)
    }

    fn subscription(&self) -> Subscription<Event> {
        Subscription::run(deliveries)
    }

    fn update(&mut self, event: Event) -> Task<Event> {
        self.handle(event);
        Task::none()
    }

    fn on_app_exit(&mut self) -> Option<Event> {
        self.handle(Event::Close);
        None
    }

    fn view(&self) -> Element<'_, Event> {
        view::view(&self.plan)
    }
}

fn main() {
    puron_settings::start();

    let (tx, rx) = unbounded();
    *DELIVERIES.lock().unwrap() = Some(rx);
    let config = Config::new(puron_config::user_root().unwrap_or_default());

    let w = window_settings();
    let settings = Settings::default()
        .size(Size::new(w.initial.0, w.initial.1))
        .size_limits(Limits::NONE.min_width(w.minimum.0).min_height(w.minimum.1));
    let _ = cosmic::app::run_single_instance::<App>(settings, Flags { config, tx });
}
