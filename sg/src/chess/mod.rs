use std::{net::{TcpListener, ToSocketAddrs}, thread::{JoinHandle, self}, sync::{mpsc, atomic::AtomicBool, Arc}, time::Duration, io::{Cursor, Read}, ops::Deref, lazy::Lazy};
use anyhow::{Result, anyhow};
use image::EncodableLayout;
use parking_lot::Mutex;
use rsa::{RsaPublicKey, pkcs8::{EncodePublicKey, PublicKeyDocument}, pkcs1::DecodeRsaPublicKey, PublicKey, PaddingScheme, RsaPrivateKey};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod message;


/// Clients is the running instance, it is both a server and a client because of the P2P
/// architecture of the protocol
pub struct Client {
    threads: Vec<(&'static str, JoinHandle<()>)>,
    stop: Arc<AtomicBool>,
}

/// Just read the value of an atomic bool, here for readability
#[inline(always)]
fn should(bl: &AtomicBool) -> bool {
    bl.load(std::sync::atomic::Ordering::Relaxed)
}

impl Client {
    pub fn new(addr: impl ToSocketAddrs) -> Result<Self> {
        let mut res = Self {
            threads: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
        };

        res.start(addr)?;

        Ok(res)
    }

    fn set_stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn spawn<F: FnOnce() -> () + Send + 'static>(&mut self, s: &'static str, f: F) {
        self.threads.push((s, thread::spawn(f)))
    }

    fn start(&mut self, addr: impl ToSocketAddrs) -> Result<()> {
        let listener = TcpListener::bind(addr)?;
        let (sender, receiver) = mpsc::channel();

        let stop = self.stop.clone();
        self.spawn("Dispatcher", move || {
            for stream in listener.incoming() {
                if should(&stop) {
                    break;
                }

                match stream {
                    Ok(stream) => { sender.send(stream).ok(); }
                    Err(err) => log::warn!("Error in stream {err:?}"),
                };
            }
        });

        let receiver = Arc::new(Mutex::new(receiver));
        for _ in 0..5 {
            let stop = self.stop.clone();
            let receiver = receiver.clone();
            self.spawn("Connection", move || {
                loop {
                    // Timeout to make sure to check for stop condition
                    let stream = receiver.lock().recv_timeout(Duration::from_millis(500));
                    
                    if let Ok(stream) = stream {
                        
                    }

                    if should(&stop) {
                        break;
                    }
                }
            });
        }

        Ok(())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Stop all threads
        self.set_stop();
        for (name, thread) in self.threads.drain(..) {
            log::debug!("Shutting down thread {name}.");
            if let Err(_) = thread.join() {
                log::warn!("Error when joining thread {name}.");
            }
        }
    }
}
