///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! [I hate myself, for making documentation.]
//!
//! ### How Quing works.
//! Quing works around 2 central structures:
//! - A [`Track`]
//! - A [`Playlist`] (grouping of [`Tracks`], with additional data)
//!
//! [`Track`]: playback::Track
//! [`Tracks`]: playback::Track
//! [`Playlist`]: playback::Playlist
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use crossbeam_channel::{RecvError, RecvTimeoutError, TryRecvError};
use rodio::{decoder::DecoderError, PlayError, StreamError};
use std::{env::VarError, io::Error as IOError};
use toml::de::Error as TOMLError;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// A module for handling and interacting with external devices.
pub mod in_out;

/// SerDe, specifically: TOML, based structure representations of the structures of [`playback`].
pub mod serde;

/// The module responsible for handling the playing of [sources]
///
/// [sources]: rodio::Source
pub mod playback;

/// Implementation utilities.
mod utilities;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
/// Errors encountered when
#[doc = env!("CARGO_PKG_NAME")]
/// interacts with [`Vec`]-esque structures.
#[cfg_attr(
	any(debug_assertions, feature = "traits"),
	derive(PartialEq, Eq, PartialOrd, Ord),
	derive(Hash)
)]
pub enum VectorError {
	/// Overflowing an index, because under-flowing an [unsigned integer] based index is impossible.
	///
	/// [unsigned integer]: usize
	OutOfBounds,

	/// As the name says.
	Empty,
}

#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
#[cfg_attr(
	any(debug_assertions, feature = "traits"),
	derive(PartialEq, Eq, PartialOrd, Ord),
	derive(Hash)
)]
#[derive(Default)]
pub enum ChannelError {
	Timeout,
	Empty,
	#[default]
	Disconnect,
}

#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
pub enum Error {
	Io(IOError),
	Decode(DecoderError),
	Play(PlayError),
	Stream(StreamError),
	Deserialise(TOMLError),
	Variable(VarError),
	Vector(VectorError),
	Channel(ChannelError),
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl From<RecvTimeoutError> for ChannelError {
	#[inline(always)]
	/// Convert from [`RecvTimoutError`] to [`Self`].
	///
	/// This is a handwritten version of [`thiserror`]'s `Error::from` attribute.
	fn from(error: RecvTimeoutError) -> Self {
		match error {
			RecvTimeoutError::Timeout => Self::Timeout,
			RecvTimeoutError::Disconnected => Self::Disconnect,
		}
	}
}

impl From<()> for ChannelError {
	#[inline(always)]
	/// Convenience to construct a [`Disconnect`].
	///
	/// [`Disconnect`]: Self.Disconnect
	fn from(_: ()) -> Self {
		Self::default()
	}
}

impl From<RecvError> for ChannelError {
	#[inline(always)]
	/// Convert any [`RecvError`] into a [`Disconnect`].
	///
	/// [`Disconnect`]: Self.Disconnect
	fn from(_: RecvError) -> Self {
		().into()
	}
}

impl From<TryRecvError> for ChannelError {
	#[inline(always)]
	/// Convert from [`TryRecvError`] to [`Self`]
	///
	/// This is a handwritten version of [`thiserror`]'s `Error::from` attribute.
	fn from(error: TryRecvError) -> Self {
		match error {
			TryRecvError::Empty => Self::Empty,
			TryRecvError::Disconnected => Self::Disconnect,
		}
	}
}

impl From<IOError> for Error {
	#[inline(always)]
	fn from(inner: IOError) -> Self {
		Self::Io(inner)
	}
}
impl From<DecoderError> for Error {
	#[inline(always)]
	fn from(inner: DecoderError) -> Self {
		Self::Decode(inner)
	}
}
impl From<PlayError> for Error {
	#[inline(always)]
	fn from(inner: PlayError) -> Self {
		Self::Play(inner)
	}
}
impl From<StreamError> for Error {
	#[inline(always)]
	fn from(inner: StreamError) -> Self {
		Self::Stream(inner)
	}
}
impl From<TOMLError> for Error {
	#[inline(always)]
	fn from(inner: TOMLError) -> Self {
		Self::Deserialise(inner)
	}
}
impl From<VarError> for Error {
	#[inline(always)]
	fn from(inner: VarError) -> Self {
		Self::Variable(inner)
	}
}
impl From<VectorError> for Error {
	#[inline(always)]
	fn from(inner: VectorError) -> Self {
		Self::Vector(inner)
	}
}
impl From<ChannelError> for Error {
	#[inline(always)]
	fn from(inner: ChannelError) -> Self {
		Self::Channel(inner)
	}
}
