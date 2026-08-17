//! Building LuaJIT into the game's Lua engine.
//!
//! The one failure mode that would silently brick a player's game is a `lua51_Win32.dll` that
//! is missing a symbol the game imports from it, so [`exports`] checks for exactly that
//! before any build is allowed near the Game Installation.

pub mod exports;
