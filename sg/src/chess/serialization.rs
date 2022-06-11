use std::{
    io::{Cursor, Read},
    ops::Deref,
};

use anyhow::{anyhow, Result};
use rsa::{
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey},
    RsaPrivateKey, RsaPublicKey,
};
use uuid::Uuid;

use super::{message::{Error, Message, Player, Signature, SignedMessage}, game::{Piece, PieceKind, Color}};

pub trait Serialize {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()>;
}

pub trait Deserialize {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self>
    where
        Self: Sized;
}

impl<const C: usize> Serialize for [u8; C] {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        bytes.extend_from_slice(self);
        Ok(())
    }
}

impl<const C: usize> Deserialize for [u8; C] {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self> {
        let mut buf = [0u8; C];
        bytes.read_exact(&mut buf)?;
        Ok(buf)
    }
}

impl<T: Serialize> Serialize for Vec<T> {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        for v in self {
            v.serialize(bytes)?;
        }
        Ok(())
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
    where
        Self: Sized,
    {
        Ok(u8::from_be_bytes(Deserialize::deserialize(bytes)?))
    }
}

impl Serialize for u32 {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        bytes.extend_from_slice(&self.to_be_bytes());
        Ok(())
    }
}

impl Deserialize for u32 {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self>
    where
        Self: Sized,
    {
        Ok(u32::from_be_bytes(Deserialize::deserialize(bytes)?))
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
    where
        Self: Sized,
    {
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
    where
        Self: Sized,
    {
        Ok(Uuid::from_bytes(Deserialize::deserialize(bytes)?))
    }
}

impl Serialize for RsaPublicKey {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        let slice = self.to_public_key_der()?;
        (slice.as_ref().len() as u64).serialize(bytes)?;
        bytes.extend_from_slice(slice.as_ref());
        Ok(())
    }
}

impl Deserialize for RsaPublicKey {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self>
    where
        Self: Sized,
    {
        let len = u64::deserialize(bytes)?;
        let mut buf = vec![0u8; len as usize];
        bytes.read_exact(&mut buf)?;
        Ok(RsaPublicKey::from_public_key_der(&buf)?)
    }
}

impl Serialize for RsaPrivateKey {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        let slice = self.to_pkcs8_der()?;
        (slice.as_ref().len() as u64).serialize(bytes)?;
        bytes.extend_from_slice(slice.as_ref());
        Ok(())
    }
}

impl Deserialize for RsaPrivateKey {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self>
    where
        Self: Sized,
    {
        let len = u64::deserialize(bytes)?;
        let mut buf = vec![0u8; len as usize];
        bytes.read_exact(&mut buf)?;
        Ok(RsaPrivateKey::from_pkcs8_der(&buf)?)
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
            Message::GameProposal {
                starting_player,
                self_player,
            } => {
                Self::GAME_PROPOSAL.serialize(bytes)?;
                starting_player.serialize(bytes)?;
                self_player.serialize(bytes)?;
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
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self> {
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
            Self::GAME_PROPOSAL => Message::GameProposal {
                starting_player: Player::deserialize(bytes)?,
                self_player: Player::deserialize(bytes)?,
            },
            Self::ERROR => Message::Error(Error::try_from(u8::deserialize(bytes)?)?),

            _ => Err(anyhow!("Unknown message type"))?,
        })
    }
}

impl Serialize for Signature {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        self.deref().serialize(bytes)
    }
}

impl Deserialize for Signature {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self> {
        Ok(<[u8; 256]>::deserialize(bytes)?.into())
    }
}

impl Serialize for SignedMessage {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        self.message.serialize(bytes)?;
        self.signature.serialize(bytes)
    }
}

impl Deserialize for SignedMessage {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self> {
        Ok(Self {
            message: Message::deserialize(bytes)?,
            signature: Signature::deserialize(bytes)?,
        })
    }
}

impl Serialize for Piece {
    fn serialize(&self, bytes: &mut Vec<u8>) -> Result<()> {
        self.kind.serialize(bytes)?;
        self.color.serialize(bytes)
    }
}

impl Deserialize for Piece {
    fn deserialize(bytes: &mut Cursor<Vec<u8>>) -> Result<Self> {
        Ok(Self {
            kind: PieceKind::deserialize(bytes)?,
            color: Color::deserialize(bytes)?
        })
    }
}
