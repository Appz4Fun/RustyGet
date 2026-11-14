//! Rust parts of nzbget, called from C++ through a C ABI (see nzbget_rs.h).
//! Each module replaces one C++ routine and must match its output byte for byte.

pub mod archive_native;
#[cfg(unix)]
pub mod articledl;
#[cfg(unix)]
pub mod articlefetcher;
pub mod binrpc;
pub mod bytes;
pub mod cleanup;
pub mod cmdline;
pub mod collection;
pub mod crc;
pub mod decode;
pub mod decoder;
pub mod deobfuscation;
pub mod directrenamer;
pub mod donorscore;
pub mod downloadinfo;
pub mod dupecoord;
pub mod dupeprobe;
pub mod dupesearch_coord;
pub mod dupesearch_nzb;
pub mod escape;
pub mod extension;
pub mod extload;
pub mod feed;
pub mod feedfilter;
pub mod ffi;
pub mod filetypes;
pub mod fsdelete;
#[cfg(unix)]
pub mod fsops;
pub mod historycoord;
pub mod json;
#[cfg(unix)]
pub mod log;
pub mod manifest;
#[cfg(unix)]
pub mod net;
pub mod nntp;
pub mod nserv;
pub mod nstring;
pub mod nzbfile;
pub mod options;
pub mod parchecker;
pub mod pardamage;
pub mod parparser;
pub mod parrenamer;
pub mod paths;
pub mod posting;
#[cfg(unix)]
pub mod postproc;
pub mod prepost;
pub mod queueeditor;
pub mod rarreader;
pub mod rarrenamer;
pub mod releasename;
pub mod renameanalyzer;
pub mod repair;
pub mod rpcparams;
pub mod rpcroute;
pub mod scanner;
pub mod scheduler;
#[cfg(unix)]
pub mod script;
#[cfg(unix)]
pub mod socket;
pub mod statmeter;
pub mod systemhealth;
pub mod text;
pub mod tlscipher;
#[cfg(unix)]
pub mod tlssock;
pub mod twin;
pub mod unpack;
pub mod url;
pub mod urlcoord;
pub mod util;
pub mod webdownload;
pub mod webserver;
pub mod webutil;
pub mod wildmask;

pub mod artwritercore;
pub mod diskstate_format;
pub mod dupefallback;
pub mod dupeidentity;
pub mod ncurses_format;
pub mod postrepair;
pub mod queuecoord;
pub(crate) mod queuesubsys_host;
pub mod streamrepair;
#[cfg(unix)]
pub mod streamrepair_coord;
pub mod xmlrpc_format;

pub mod deadpostings;
pub mod dupewatch;
pub mod streamcompare;
pub mod streampatchparts;

pub mod dupepurge;

#[cfg(test)]
mod allocation_probe;
pub mod purgejournal;

pub mod entrypoint;
pub mod lifecycle;
pub mod memory_pressure;
