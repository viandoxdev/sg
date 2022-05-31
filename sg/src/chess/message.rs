use anyhow::{anyhow, Result};
use mio::net::TcpStream;
use rsa::{PaddingScheme, PublicKey, RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use std::{
    fmt::{Debug, Display},
    ops::{BitXor, Deref}, io::{Write, Read, Cursor},
};

use crate::numeric_enum;

use super::serialization::{Serialize, Deserialize};

numeric_enum! {
    // An error in the game's processing, usally a fatal one
    pub enum Error: u8 {
        IllegalMove = 0,
        Disagreement = 1,
        UnexpectedMessage = 2,
    }
    pub enum Player: u8 {
        // The peer who sent the game request
        Requester = 0,
        // The peer who received it
        Requestee = 1,
    }
}

impl Player {
    pub fn new_random() -> Player {
        if rand::random() {
            Player::Requester
        } else {
            Player::Requestee
        }
    }
}

impl BitXor for Player {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self::Output {
        Self::try_from(self as u8 ^ rhs as u8).unwrap()
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Request a new game from a peer
    NewGameRequest {
        game_id: Uuid,
        /// Client's public key
        public_key: RsaPublicKey,
    },
    /// Accept a new game from a peer
    NewGameApproval {
        game_id: Uuid,
        /// Public key of peer
        public_key: RsaPublicKey,
    },
    /// Proposal for the game formalities, each party sends a proposal, then mix theirs with the
    /// other's to get the final settings
    GameProposal {
        /// Who is the starting player (who will play white)
        starting_player: Player,
        /// Who is the sender of the message saying they are
        self_player: Player,
    },
    Error(Error),
}

impl Message {
    pub const NEW_GAME_REQUEST: u8 = 0;
    pub const NEW_GAME_APPROVAL: u8 = 1;
    pub const GAME_PROPOSAL: u8 = 2;
    pub const ERROR: u8 = 3;

    fn hash(&self) -> Result<sha2::digest::Output<Sha256>> {
        let mut buf = Vec::new();
        self.serialize(&mut buf)?;
        Ok(Sha256::digest(&buf))
    }

    pub fn sign(self, key: &RsaPrivateKey) -> Result<SignedMessage> {
        let hash = self.hash()?;
        let padding = PaddingScheme::new_pkcs1v15_sign(Some(rsa::Hash::SHA2_256));
        let sig = key.sign(padding, &hash)?;
        Ok(SignedMessage {
            message: self,
            signature: Signature::try_from(sig)?,
        })
    }

    pub fn read(stream: &mut TcpStream) -> Result<Message> {
        let mut buf = vec![0; 1024];
        let read = stream.read(&mut buf)?;
        if read == 0 {
            return Err(anyhow!("Got 0 bytes from read"));
        }
        let mut bytes = Cursor::new(buf);
        Message::deserialize(&mut bytes)
    }

    pub fn send(&self, stream: &mut TcpStream) -> Result<()> {
        let mut buf = Vec::new();
        self.serialize(&mut buf)?;
        stream.write_all(&buf)?;
        Ok(())
    }
}

pub struct Signature {
    sig: [u8; 256],
}

impl Deref for Signature {
    type Target = [u8; 256];
    fn deref(&self) -> &Self::Target {
        &self.sig
    }
}

impl Debug for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signature({self})")
    }
}

impl Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.sig
                .iter()
                .map(|byte| format!("{byte:x}"))
                .collect::<String>()
        )
    }
}

impl TryFrom<Vec<u8>> for Signature {
    type Error = anyhow::Error;
    fn try_from(mut value: Vec<u8>) -> Result<Self, Self::Error> {
        if value.len() >= 256 {
            let mut arr = [0u8; 256];
            arr.swap_with_slice(&mut value[0..256]);
            Ok(Self { sig: arr })
        } else {
            Err(anyhow!("Not enough bytes to build signature"))
        }
    }
}

impl From<[u8; 256]> for Signature {
    fn from(v: [u8; 256]) -> Self {
        Self { sig: v }
    }
}

#[derive(Debug)]
pub struct SignedMessage {
    pub message: Message,
    pub signature: Signature,
}

impl SignedMessage {
    pub fn verify_signature(&self, key: &RsaPublicKey) -> Result<()> {
        let hash = self.message.hash()?;
        let padding = PaddingScheme::new_pkcs1v15_sign(Some(rsa::Hash::SHA2_256));
        key.verify(padding, &hash, &*self.signature)?;
        Ok(())
    }

    /// Get the Message out of the SignedMessage if the signature is verrified
    pub fn verify_and_unwrap(self, key: &RsaPublicKey) -> Result<Message> {
        self.verify_signature(key)?;
        Ok(self.message)
    }

    pub fn read(stream: &mut TcpStream) -> Result<SignedMessage> {
        // TODO: better buffer size management, for now we just assume 1kb is enough
        let mut buf = vec![0; 1024];
        let read = stream.read(&mut buf)?;
        if read == 0 {
            return Err(anyhow!("Got 0 bytes from read"));
        }
        let mut bytes = Cursor::new(buf);
        SignedMessage::deserialize(&mut bytes)
    }

    pub fn send(&self, stream: &mut TcpStream) -> Result<()> {
        let mut buf = Vec::new();
        self.serialize(&mut buf)?;
        stream.write_all(&buf)?;
        Ok(())
    }
}

impl Deref for SignedMessage {
    type Target = Message;
    fn deref(&self) -> &Self::Target {
        &self.message
    }
}
