//! Contains the types and functions needed to use the Decentraland RPC implementation.
pub mod parse;
// proto file definition doesn't have a package name, so it defaults to "_"
include!(concat!(env!("OUT_DIR"), "/_.rs"));
