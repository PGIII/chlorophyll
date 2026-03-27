use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use crate::event::{AppEvent, Event, EventHandler};
use crate::log_widget::LogState;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use sensor_server::{process_packets, DataEntry, MULTICAST_ADDR, PORT};

/// Re-join the multicast group every ~10 s to send a fresh IGMP membership report.
const REJOIN_TICKS: u64 = 300;
use tokio::net::UdpSocket;
use tracing::*;

/// Application.
#[derive(Debug)]
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Counter.
    pub counter: u8,
    /// Event handler.
    pub events: EventHandler,

    pub socket: Option<UdpSocket>,
    pub last_reading: Vec<DataEntry>,
    pub log_state: LogState,

    /// Known devices: sensor_id → source socket address
    pub known_devices: HashMap<u128, SocketAddr>,
    tick_count: u64,
}

impl Default for App {
    fn default() -> Self {
        Self {
            running: true,
            counter: 0,
            events: EventHandler::new(),
            socket: None,
            last_reading: Vec::new(),
            log_state: LogState::new(true),
            known_devices: HashMap::new(),
            tick_count: 0,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new(log_state: LogState) -> Self {
        Self {
            running: true,
            counter: 0,
            events: EventHandler::new(),
            socket: None,
            last_reading: Vec::new(),
            log_state,
            known_devices: HashMap::new(),
            tick_count: 0,
        }
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
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
        self.tick_count = self.tick_count.wrapping_add(1);

        if let Some(sock) = &self.socket {
            // Periodically leave and rejoin the multicast group to send a fresh IGMP
            // membership report. A plain join on an already-joined socket is rejected
            // with EADDRINUSE; leave+join is the only way to trigger a new report and
            // prevent the router from timing out our group membership.
            if self.tick_count % REJOIN_TICKS == 0 {
                sock.leave_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED).ok();
                if let Err(e) = sock.join_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED) {
                    error!("Failed to rejoin multicast group: {e}");
                }
            }

            if let Err(e) =
                process_packets(sock, &mut self.known_devices, &mut self.last_reading).await
            {
                error!("process_packets error: {e}");
            }
        } else {
            info!("No socket, setting up");
            let socket_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT);
            match UdpSocket::bind(socket_addr).await {
                Ok(sock) => {
                    if let Err(e) = sock.join_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED) {
                        error!("Couldn't join multicast group: {e}");
                        return;
                    }
                    info!("Joined multicast {}:{}", MULTICAST_ADDR, PORT);
                    self.socket = Some(sock);
                }
                Err(e) => {
                    error!("Couldn't open socket: {e}");
                }
            }
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
