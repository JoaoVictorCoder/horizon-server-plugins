//! Forja's library, without the plugin: what packs define ([`content`]),
//! custom blocks as note block states ([`blocks`]), the resource pack built
//! from them ([`pack`], [`zip`], [`sha1`]) and the server that hands it out
//! ([`http`]). The Forja plugin (`forja/`) runs them; another plugin can
//! read its own content with them.

pub mod blocks;
pub mod content;
pub mod http;
pub mod pack;
pub mod sha1;
pub mod zip;
