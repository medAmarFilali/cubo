//! Cubo - A containerization tool focused on isolation and security
//! 
//! This library provides core fonctionality for container management,
//! including container lifecycle, image storage, and Cubofile parsing.

pub mod error;
pub mod commands;
pub mod cli;
pub mod container;

pub use error::{CuboError, Result};