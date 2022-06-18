use anyhow::{anyhow, Result};
use mio::{
    net::{TcpListener, TcpStream},
    Events, Interest, Poll, Token,
};
use rsa::{RsaPrivateKey, RsaPublicKey};
use std::{
    collections::HashMap,
    io::{Cursor, Write},
    net::ToSocketAddrs,
    sync::{
        atomic::AtomicBool,
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use uuid::Uuid;

use parking_lot::{Mutex, RwLock};

use crate::chess::message::Error;

use self::{
    game::Color,
    message::{Message, Player, SignedMessage},
    serialization::{Deserialize, Serialize},
};

pub mod game;
pub mod message;
pub mod numeric_enum;
pub mod serialization;

#[derive(Clone)]
struct Game {
    self_player: Player,
    self_color: Color,
    peer_public_key: RsaPublicKey,
}

/// Clients is the running instance, it is both a server and a client because of the P2P
/// architecture of the protocol
pub struct Client {
    threads: Vec<(&'static str, JoinHandle<()>)>,
    stop: Arc<AtomicBool>,
    session_producer: Sender<TcpStream>,
    game_producer: Sender<TcpStream>,
    private_key: RsaPrivateKey,
    public_key: RsaPublicKey,
    game: Arc<RwLock<Option<Game>>>,
    ongoing_game_requests: Arc<Mutex<HashMap<Uuid, Instant>>>, // all ongoing game requests mapped to a timestamp of when they were sent.
}

/// Just read the value of an atomic bool, here for readability
#[inline(always)]
fn should(bl: &AtomicBool) -> bool {
    bl.load(std::sync::atomic::Ordering::Relaxed)
}

fn get_saved_key() -> Result<RsaPrivateKey> {
    let mut bytes = Cursor::new(std::fs::read("private_key")?);
    RsaPrivateKey::deserialize(&mut bytes)
}

fn create_and_save_key() -> RsaPrivateKey {
    let private_key =
        RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("Error on key generation");
    let mut buf = Vec::new();

    // try to serialize the key
    if let Err(err) = private_key.serialize(&mut buf) {
        log::warn!("Error when serializing private key ({err})");
        return private_key;
    }
    // if serialization worked, try to write it
    if let Err(err) = std::fs::write("private_key", &buf) {
        log::warn!("Error when writing private key to file ({err})");
    }
    private_key
}

fn get_key() -> RsaPrivateKey {
    get_saved_key().unwrap_or_else(|_| create_and_save_key())
}

impl Client {
    const CONNECTION: Token = Token(0);

    pub fn new(addr: impl ToSocketAddrs) -> Result<Self> {
        let private_key = get_key();
        let public_key = RsaPublicKey::from(private_key.clone());
        let (session_producer, session_receiver) = mpsc::channel();
        let (game_producer, game_receiver) = mpsc::channel();
        let mut res = Self {
            threads: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
            session_producer,
            game_producer,
            private_key,
            public_key,
            game: Arc::new(RwLock::new(None)),
            ongoing_game_requests: Arc::new(Mutex::new(HashMap::new())),
        };

        res.start(addr, session_receiver, game_receiver)?;

        Ok(res)
    }

    pub fn get_keys(&self) -> (&RsaPrivateKey, &RsaPublicKey) {
        (&self.private_key, &self.public_key)
    }

    fn set_stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn spawn<F: FnOnce() + Send + 'static>(&mut self, s: &'static str, f: F) {
        self.threads.push((s, thread::spawn(f)))
    }

    fn start(
        &mut self,
        addr: impl ToSocketAddrs,
        session_receiver: Receiver<TcpStream>,
        game_receiver: Receiver<TcpStream>,
    ) -> Result<()> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| anyhow!("Can't get socket address"))?;
        let listener = TcpListener::bind(addr)?;

        let stop = self.stop.clone();
        let sender = self.session_producer.clone();
        self.spawn("Dispatcher", move || {
            loop {
                if should(&stop) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        sender.send(stream).ok();
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        // no connection ready, loop
                    }
                    Err(err) => {
                        log::error!("Unhandled error on accept connection ({err})");
                        break;
                    }
                }
            }
        });

        let receiver = Arc::new(Mutex::new(session_receiver));
        for id in 0..5 {
            let stop = self.stop.clone();
            let receiver = receiver.clone();
            let key = self.public_key.clone();
            let game = self.game.clone();
            let game_producer = self.game_producer.clone();
            let ongoing_game_requests = self.ongoing_game_requests.clone();
            self.spawn("Session", move || {
                loop {
                    // Timeout to make sure to check for stop condition
                    let stream = receiver.lock().recv_timeout(Duration::from_millis(100));

                    if let Ok(stream) = stream {
                        let id = format!("{} | {id}", stream.local_addr().unwrap());
                        log::trace!(
                            "{id} Received session (from {})",
                            stream.peer_addr().unwrap()
                        );
                        if let Err(err) = handle_session(
                            &id,
                            &stop,
                            stream,
                            &key,
                            &game_producer,
                            &game,
                            &ongoing_game_requests,
                        ) {
                            log::error!("{id} Error in session: {err}");
                        };
                    }

                    if should(&stop) {
                        break;
                    }
                }
            });
        }

        let stop = self.stop.clone();
        let game = self.game.clone();
        let (private_key, _) = self.get_keys();
        let private_key = private_key.clone();
        self.spawn("Game", move || {
            'outer: loop {
                // try to get a stream, in a loop to prediodically check for stop
                let mut stream;
                loop {
                    stream = game_receiver.recv_timeout(Duration::from_millis(100)).ok();

                    if stream.is_some() {
                        break;
                    }

                    if should(&stop) {
                        break 'outer;
                    }
                }

                if let Some(mut stream) = stream {
                    if let Err(err) = handle_game_stream(&game, &mut stream, &stop, &private_key) {
                        log::warn!("Error when starting game: {err}");
                    }
                }
            }
        });

        Ok(())
    }

    pub fn request_game(&mut self, addr: impl ToSocketAddrs) -> Result<()> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| anyhow!("Can't get socket address"))?;
        let stream = std::net::TcpStream::connect(addr)?;
        let mut stream = TcpStream::from_std(stream); // Turn blocking std::net::TcpStream into non blocking mio one
        let id = Uuid::new_v4();
        let request = Message::NewGameRequest {
            game_id: id,
            public_key: self.public_key.clone(),
        };
        self.ongoing_game_requests.lock().insert(id, Instant::now());
        let mut buf = Vec::new();
        request.serialize(&mut buf)?;
        stream.write_all(&buf)?;
        log::trace!(
            "Sent game request to {addr} (from {})",
            stream.local_addr().unwrap()
        );
        self.session_producer.send(stream)?;
        Ok(())
    }
}

fn handle_session(
    id: &str,               // identifying string (used in logs)
    stop: &Arc<AtomicBool>, // wether or not this thread should stop
    mut stream: TcpStream,
    key: &RsaPublicKey,                // our public key
    game_producer: &Sender<TcpStream>, // a Sender used to hand the stream to the next thread
    game: &Arc<RwLock<Option<Game>>>, // a Game struct (game info) in its proper rust thread safe form
    ongoing_game_requests: &Arc<Mutex<HashMap<Uuid, Instant>>>,
) -> Result<()> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(32);
    const READ: Token = Token(0);
    poll.registry()
        .register(&mut stream, READ, Interest::READABLE)?;

    // wait for read or stop if needed
    while poll
        .poll(&mut events, Some(Duration::from_millis(100)))
        .is_err()
    {
        if should(stop) {
            return Ok(());
        }
    }

    let msg = Message::read(&mut stream)?;
    match msg {
        Message::NewGameRequest {
            game_id,
            public_key,
        } => {
            log::debug!(
                "{id} Received game request (from {})",
                stream.peer_addr().unwrap()
            );
            // TODO: ask for user if they accept the game
            if game.read().is_none() {
                Message::NewGameApproval {
                    game_id,
                    public_key: key.clone(),
                }
                .send(&mut stream)?;

                game.write().replace(Game {
                    peer_public_key: public_key,
                    self_player: Player::Requestee,
                    self_color: Color::Black, // doesn't matter, will be overwritten
                });

                game_producer.send(stream)?; // Pass onto the next thread
            }
        }
        Message::NewGameApproval {
            game_id,
            public_key,
        } => {
            // prune all outdated requests
            ongoing_game_requests.lock().retain(|_, ts| {
                Instant::now().saturating_duration_since(*ts) < Duration::from_secs(600)
            });
            // only start a game if the approval is for a game we know we requested, this is to
            // avoid a client just sending NewGameApproval message with random ids from getting accepted by every other peer.
            if ongoing_game_requests.lock().contains_key(&game_id) {
                log::debug!("{id} Approved of game from {}", stream.peer_addr().unwrap());
                game.write().replace(Game {
                    peer_public_key: public_key,
                    self_player: Player::Requester,
                    self_color: Color::Black, // doesn't matter, will be overwritten
                });
                game_producer.send(stream)?;
            }
        }
        Message::Error(err) => {
            log::error!("Received error message from peer: {err:?}");
            return Err(anyhow!("Unexpected message"));
        }
        _ => {
            Message::Error(Error::UnexpectedMessage).send(&mut stream)?;
            return Err(anyhow!("Unexpected message"));
        }
    }
    Ok(())
}

fn handle_game_stream(
    game: &Arc<RwLock<Option<Game>>>,
    stream: &mut TcpStream,
    stop: &Arc<AtomicBool>,
    private_key: &RsaPrivateKey,
) -> Result<()> {
    let mut peek_buffer = [0; 256];
    // Theses have to be filled in for the stream to reach this thread
    let peer_key = game.read().as_ref().unwrap().peer_public_key.clone();
    let self_player = game.read().as_ref().unwrap().self_player;

    let id = format!("[{}]", stream.local_addr().unwrap());

    // Chose each player's color:

    // Choose a random starting player (= player with the white color)
    let starting_player = Player::new_random();
    // build a game proposal with it
    let prop = Message::GameProposal {
        starting_player,
        self_player,
    };

    // Send proposal
    prop.sign(private_key)?.send(stream)?;
    // Wait for peer's proposal
    while stream.peek(&mut peek_buffer).unwrap_or(0) == 0 {
        if should(stop) {
            return Ok(());
        }
    }
    // Read peer's response
    let peer_prop = SignedMessage::read(stream)?.verify_and_unwrap(&peer_key)?;

    // check if peer's response is indeed a proposal
    if let Message::GameProposal {
        starting_player: peer_starting_player,
        self_player: peer_player, // who the peer is saying they are
    } = peer_prop
    {
        // Check if we have a disagreement on who is who.
        if peer_player == self_player {
            Message::Error(Error::Disagreement)
                .sign(private_key)?
                .send(stream)?;
            return Ok(()); // The error comes from the peer, not us, so return Ok
        }
        // Compute color from starting_player, starting_player being
        // self_starting_player ^ peer_starting_player
        game.write().as_mut().unwrap().self_color =
            if starting_player ^ peer_starting_player == self_player {
                Color::White
            } else {
                Color::Black
            };
    } else {
        // It isn't, error out
        Message::Error(Error::UnexpectedMessage)
            .sign(private_key)?
            .send(stream)?;
        return Ok(());
    }

    // The game can start
    let game = game.read().as_ref().unwrap().clone();
    log::trace!("{id} Got color: {:?}", game.self_color);
    Ok(())
}

impl Drop for Client {
    fn drop(&mut self) {
        // Stop all threads
        self.set_stop();
        for (name, thread) in self.threads.drain(..) {
            log::debug!("Shutting down thread {name}.");
            if thread.join().is_err() {
                log::warn!("Error when joining thread {name}.");
            }
        }
    }
}
