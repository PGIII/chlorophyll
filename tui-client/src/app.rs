use crate::event::{AppEvent, Event, EventHandler};
use crate::log_widget::LogState;
use chlorophyll_client::db::Db;
use chlorophyll_client::{ClientConfig, Reading, SensorClient};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use tokio::sync::broadcast;
use tracing::*;

/// Keep up to ~24 h of readings at ~1 reading/sensor/5 s (generous headroom).
const MAX_READINGS: usize = 100_000;

/// Application.
#[derive(Debug)]
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Counter.
    pub counter: u8,
    /// Event handler.
    pub events: EventHandler,

    pub client: Option<SensorClient>,
    pub readings_rx: Option<broadcast::Receiver<Reading>>,
    pub db: Option<Db>,
    pub last_reading: Vec<Reading>,
    pub log_state: LogState,
}

impl Default for App {
    fn default() -> Self {
        Self {
            running: true,
            counter: 0,
            events: EventHandler::new(),
            client: None,
            readings_rx: None,
            db: None,
            last_reading: Vec::new(),
            log_state: LogState::new(true),
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new(log_state: LogState) -> Self {
        Self { log_state, ..Self::default() }
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        let db_path =
            std::env::var("CHLOROPHYLL_DB").unwrap_or_else(|_| "chlorophyll.db".to_string());
        match Db::open(&db_path).await {
            Ok(db) => {
                info!("Database opened at {db_path}");
                self.db = Some(db);
            }
            Err(e) => error!("Failed to open database at {db_path}: {e}"),
        }

        match SensorClient::start(ClientConfig::default()) {
            Ok(client) => {
                self.readings_rx = Some(client.subscribe());
                self.client = Some(client);
            }
            Err(e) => error!("Failed to start sensor client: {e}"),
        }

        while self.running {
            terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;
            match self.events.next().await? {
                Event::Tick => self.tick().await,
                Event::Crossterm(event) => match event {
                    crossterm::event::Event::Key(key_event)
                        if key_event.kind == KeyEventKind::Press =>
                    {
                        self.handle_key_events(key_event)?
                    }
                    _ => {}
                },
                Event::App(app_event) => match app_event {
                    AppEvent::Increment => self.increment_counter(),
                    AppEvent::Decrement => self.decrement_counter(),
                    AppEvent::Quit => self.quit(),
                },
            }
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    pub fn handle_key_events(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => self.events.send(AppEvent::Quit),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.events.send(AppEvent::Quit)
            }
            KeyCode::Char('r' | 'R') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.last_reading.clear();
            }
            KeyCode::Char('L') if key_event.modifiers == KeyModifiers::SHIFT => {
                self.log_state.toggle();
            }
            KeyCode::Up => {
                if self.log_state.enabled {
                    self.log_state.scroll_up(1);
                }
            }
            KeyCode::Down => {
                if self.log_state.enabled {
                    self.log_state.scroll_down(1);
                }
            }
            KeyCode::PageUp => {
                if self.log_state.enabled {
                    self.log_state.scroll_up(10);
                }
            }
            KeyCode::PageDown => {
                if self.log_state.enabled {
                    self.log_state.scroll_down(10);
                }
            }
            KeyCode::Right => self.events.send(AppEvent::Increment),
            KeyCode::Left => self.events.send(AppEvent::Decrement),
            _ => {}
        }
        Ok(())
    }

    /// Handles the tick event of the terminal.
    pub async fn tick(&mut self) {
        self.counter = self.counter.wrapping_add(1);

        let Some(rx) = self.readings_rx.as_mut() else { return };

        let mut new_readings = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(reading) => new_readings.push(reading),
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    warn!("readings channel lagged, dropped {n} messages");
                }
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }

        if let Some(db) = &self.db {
            for reading in &new_readings {
                if let Err(e) = db.insert_reading(reading).await {
                    error!("DB insert error: {e}");
                }
            }
        }

        self.last_reading.extend(new_readings);
        if self.last_reading.len() > MAX_READINGS {
            let excess = self.last_reading.len() - MAX_READINGS;
            self.last_reading.drain(..excess);
        }
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn increment_counter(&mut self) {
        self.counter = self.counter.saturating_add(1);
    }

    pub fn decrement_counter(&mut self) {
        self.counter = self.counter.saturating_sub(1);
    }
}
