use std::{io::{Cursor, Read}, ops::Deref};

use anyhow::{anyhow, Result};
use rsa::{RsaPublicKey, RsaPrivateKey, PaddingScheme, PublicKey, pkcs1::{DecodeRsaPublicKey, EncodeRsaPublicKey}};
use sha2::{Sha256, Digest};
use uuid::Uuid;

#[derive(Clone, Copy)]
enum Error {
    IllegalMove = 0,
}

impl TryFrom<u8> for Error {
    type Error = anyhow::Error;
    fn try_from(value: u8) -> Result<Self> {
        Ok(match value {
            0 => Error::IllegalMove,
            _ => Err(anyhow!("Can't convert u8 to error"))?,
        })
    }
}

enum Message {
    NewGameRequest {
        game_id: Uuid,
        /// Client's public key
        public_key: RsaPublicKey,
    },
    NewGameApproval {
        game_id: Uuid,
        /// Public key of peer
        public_key: RsaPublicKey,
    },
    Error(Error),
}

trait Serialize {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()>;
}

trait Deserialize {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self> where Self: Sized;
}

impl<const C: usize> Deserialize for [u8; C] {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self> {
        let mut buf = [0u8; C];
        bytes.read_exact(&mut buf)?;
        Ok(buf)
    }
}

impl Serialize for u8 {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        bytes.push(*self);
        Ok(())
    }
}

impl Deserialize for u8 {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self>
    where Self: Sized {
        Ok(u8::from_be_bytes(Deserialize::deserialize(bytes)?))
    }
}

impl Serialize for u64 {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        bytes.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl Deserialize for u64 {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self>
    where Self: Sized {
        Ok(u64::from_be_bytes(Deserialize::deserialize(bytes)?))
    }
}

impl Serialize for Uuid {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        bytes.extend_from_slice(self.as_bytes());
        Ok(())
    }
}

impl Deserialize for Uuid {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self>
    where Self: Sized {
        Ok(Uuid::from_bytes(Deserialize::deserialize(bytes)?))
    }
}

impl Serialize for RsaPublicKey {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        let slice = self.to_pkcs1_der()?;
        (slice.as_ref().len() as u64).serialize(bytes)?;
        bytes.extend_from_slice(slice.as_ref());
        Ok(())
    }
}

impl Deserialize for RsaPublicKey {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self>
    where Self: Sized {
        let len = u64::deserialize(bytes)?;
        let mut buf = vec![0u8; len as usize];
        bytes.read_exact(&mut buf)?;
        Ok(RsaPublicKey::from_pkcs1_der(&buf)?)
    }
}

impl Message {
    const NEW_GAME_REQUEST: u8 = 0;
    const NEW_GAME_APPROVAL: u8 = 1;
    const ERROR: u8 = 2;

    fn hash(&self) -> Result<sha2::digest::Output<Sha256>> {
        let mut buf = Vec::new();
        self.serialize(&mut buf)?;
        Ok(Sha256::digest(&buf))
    }

    fn sign(self, key: RsaPrivateKey) -> Result<SignedMessage> {
        let hash = self.hash()?;
        let padding = PaddingScheme::new_pkcs1v15_sign(Some(rsa::Hash::SHA2_256));
        let sig = key.sign(padding, &hash)?;
        Ok(SignedMessage {
            message: self,
            signature: sig,
        })
    }
}

impl Serialize for Message {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        match self {
            Message::NewGameRequest { game_id, public_key } => {
                Self::NEW_GAME_REQUEST.serialize(bytes)?;
                game_id.serialize(bytes)?;
                public_key.serialize(bytes)?;
            }
            Message::NewGameApproval { game_id, public_key } => {
                Self::NEW_GAME_APPROVAL.serialize(bytes)?;
                game_id.serialize(bytes)?;
                public_key.serialize(bytes)?;
            }
            Message::Error(err) => {
                Self::ERROR.serialize(bytes)?;
                (*err as u8).serialize(bytes)?;
            }
        }
        Ok(())
    }
}

impl Deserialize for Message {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self>
    where Self: Sized {
        let code = u8::deserialize(bytes)?;
        Ok(match code {
            Self::NEW_GAME_REQUEST => Message::NewGameRequest {
                game_id: Uuid::deserialize(bytes)?,
                public_key: RsaPublicKey::deserialize(bytes)?,
            },
            Self::NEW_GAME_APPROVAL => Message::NewGameApproval {
                game_id: Uuid::deserialize(bytes)?,
                public_key: RsaPublicKey::deserialize(bytes)?,
            },
            Self::ERROR => Message::Error(Error::try_from(u8::deserialize(bytes)?)?),

            _ => Err(anyhow!("Unknown message type"))?,
        })
    }
}

struct SignedMessage {
    message: Message,
    signature: Vec<u8>,
}

impl SignedMessage {
    fn verify_signature(&self, key: RsaPublicKey) -> Result<()> {
        let hash = self.message.hash()?;
        let padding = PaddingScheme::new_pkcs1v15_sign(Some(rsa::Hash::SHA2_256));
        key.verify(padding, &hash, &self.signature)?;
        Ok(())
    }
}

impl Deref for SignedMessage {
    type Target = Message;
    fn deref(&self) -> &Self::Target {
        &self.message
    }
}
