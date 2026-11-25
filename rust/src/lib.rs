//! RustyGet: a Usenet downloader descended from nzbget, written in Rust.
//!
//! The `rustyget` binary (`rust/entry.rs`) is a thin `main` over this crate.
//! Many modules still keep nzbget's behavior byte for byte and some keep
//! C-shaped signatures from the port; those are being made idiomatic.

pub mod archive_native;
#[cfg(unix)]
pub(crate) mod articledl;
#[cfg(unix)]
pub(crate) mod articlefetcher;
pub mod binrpc;
pub mod bytes;
pub(crate) mod cstr;
#[cfg(unix)]
mod callback_metrics;
pub mod cleanup;
pub mod cmdline;
pub(crate) mod collection;
pub mod configfile;
pub mod firstrun;
pub mod crc;
pub mod decode;
pub mod deadvolume;
pub mod decoder;
pub mod deobfuscation;
pub(crate) mod directrenamer;
pub(crate) mod donorscore;
pub mod downloadinfo;
pub mod dupecoord;
pub mod fleethold;
pub mod fleetprobe;
pub(crate) mod dupeprobe;
pub(crate) mod dupesearch_coord;
pub mod dupesearch_nzb;
pub mod escape;
pub mod feed;
pub(crate) mod feedfilter;
pub mod ffi;
pub mod filetypes;
pub mod fsdelete;
#[cfg(unix)]
pub(crate) mod fsops;
pub(crate) mod historycoord;
#[cfg(unix)]
pub(crate) mod jobthread;
pub mod json;
pub(crate) mod light_pool;
#[cfg(unix)]
pub mod log;
#[cfg(unix)]
pub(crate) mod maintenance;
#[cfg(unix)]
pub mod net;
pub mod nntp;
pub mod nserv;
pub mod nzbfile;
pub mod options;
pub mod parchecker;
pub(crate) mod liveparlog;
pub(crate) mod pardamage;
pub(crate) mod parpreset;
mod parlive;
pub mod parparser;
pub mod parrenamer;
pub mod paths;
pub(crate) mod posting;
#[cfg(unix)]
pub mod postproc;
pub(crate) mod prepost;
pub(crate) mod progressive_unpack;
pub(crate) mod livefeed;
#[cfg(unix)]
pub(crate) mod procrun;
pub(crate) mod queueeditor;
pub(crate) mod rarreader;
pub(crate) mod rarrenamer;
pub mod releasename;
pub mod renameanalyzer;
pub(crate) mod repair;
pub mod rpcparams;
pub(crate) mod rpcroute;
pub(crate) mod scanner;
pub(crate) mod scheduler;
#[cfg(unix)]
pub mod socket;
pub mod statmeter;
pub mod status;
pub(crate) mod certtools;
pub mod systemhealth;
pub mod backbones;
pub(crate) mod addrpick;
pub mod hostlookup;
pub(crate) mod idgen;
pub mod sysmetrics;
pub mod text;
pub mod tlscipher;
#[cfg(unix)]
pub(crate) mod tlssock;
pub mod twin;
pub(crate) mod twinfields;
pub mod unpack;
pub(crate) mod unpacklimits;
pub(crate) mod url;
pub mod urlcoord;
pub mod util;
pub(crate) mod webdownload;
pub mod webserver;
pub mod webutil;
pub mod wildmask;

pub mod artwritercore;
pub mod diskstate_format;
pub(crate) mod donorkind;
pub mod dupefallback;
pub mod dupeidentity;
pub(crate) mod ncurses_format;
pub mod postrepair;
#[cfg(unix)]
pub(crate) mod qarticle;
#[cfg(unix)]
pub(crate) mod qfile;
#[cfg(unix)]
pub mod qnzb;
pub(crate) mod qdup;
#[cfg(unix)]
pub mod qhist;
pub(crate) mod qlist;
pub(crate) mod qqueue;
pub mod queuecoord;
pub(crate) mod queuesubsys_host;
pub mod sevenzraw;
pub mod streamrepair;
#[cfg(unix)]
pub(crate) mod streamrepair_coord;
pub mod xmlrpc_format;

pub mod deadpostings;
pub mod dupeintake;
pub mod dupewatch;
pub mod streamcompare;
pub mod streampatchparts;

pub mod dupepurge;

#[cfg(test)]
mod allocation_probe;
pub mod purgejournal;

#[cfg(unix)]
pub mod daemon;
pub mod entrypoint;
pub mod lifecycle;
pub mod memory_pressure;
