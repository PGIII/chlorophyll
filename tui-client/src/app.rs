use std::net::{Ipv4Addr, SocketAddrV4};

use crate::event::{AppEvent, Event, EventHandler};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::DefaultTerminal;
use tokio::net::UdpSocket;

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
    pub last_msg: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            running: true,
            counter: 0,
            events: EventHandler::new(),
            socket: None,
            last_msg: None,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        while self.running {
            terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;
            match self.events.next().await? {
                Event::Tick => self.tick().await,
                Event::Crossterm(event) => match event {
                    crossterm::event::Event::Key(key_event)
                        if key_event.kind == crossterm::event::KeyEventKind::Press =>
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
            KeyCode::Right => self.events.send(AppEvent::Increment),
            KeyCode::Left => self.events.send(AppEvent::Decrement),
            // Other handlers you could add here.
            _ => {}
        }
        Ok(())
    }

    /// Handles the tick event of the terminal.
    ///
    /// The tick event is where you can update the state of your application with any logic that
    /// needs to be updated at a fixed frame rate. E.g. polling a server, updating an animation.
    pub async fn tick(&mut self) {
        if let Some(sock) = &self.socket {
            let mut buf = [0u8; 1500];
            match sock.try_recv_from(&mut buf) {
                Ok((len, _src)) => {
                    self.last_msg = Some(String::from_utf8_lossy(&buf[..len]).into_owned());
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return;
                }
                Err(e) => {
                    //real error
                    eprintln!("Error reading {e}");
                    return;
                }
            }
        } else {
            //setup socket
            // Multicast group and port
            let multicast_addr = Ipv4Addr::new(239, 0, 0, 1); // Example multicast address
            let port = 5000;

            // Bind to any address on the given port
            let socket_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
            match UdpSocket::bind(socket_addr).await {
                Ok(sock) => {
                    // Join the multicast group on the default interface (0.0.0.0)
                    if let Err(e) = sock.join_multicast_v4(multicast_addr, Ipv4Addr::UNSPECIFIED) {
                        eprintln!("Couldn't join multicastg group {e}");
                        return;
                    }
                    //println!("Listening for multicast on {}:{}", multicast_addr, port);
                    self.socket = Some(sock);
                }
                Err(e) => {
                    eprintln!("Couldn't open socket {e}");
                    return;
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
