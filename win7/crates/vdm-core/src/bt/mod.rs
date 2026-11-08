//! BitTorrent engine (Motrix / Gopeed headline feature).
//!
//! Modules:
//! - [`bencode`]: BEP-3 codec.
//! - [`metainfo`]: .torrent parsing/creation, piece<->file mapping.
//! - [`magnet`]: magnet URI parsing (BEP-53 style).
//! - [`pieces`]: piece store, rarest-first picker, per-file priorities.
//! - [`tracker`]: HTTP tracker client + in-process tracker server.
//! - [`peer`]: peer wire protocol (BEP-3) + extension handshake (BEP-10) +
//!   ut_metadata (BEP-9).
//! - [`seeder`]: seeding server (also used as the test harness and by
//!   `velocitydm seed`).
//! - [`engine`]: download orchestration (torrent & magnet), resume, events.

pub mod bencode;
pub mod engine;
pub mod magnet;
pub mod metainfo;
pub mod peer;
pub mod pieces;
pub mod seeder;
pub mod tracker;

pub use engine::{download_blocking, BtEvent, BtOptions, BtSpec, DownloadedTorrent};
pub use magnet::Magnet;
pub use metainfo::{create_torrent, Metainfo};

pub use pieces::{bitfield_encode, bitfield_has, PieceStore, BLOCK_LEN};
