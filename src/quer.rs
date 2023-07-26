use std::io;
use tui::{
	Terminal,
	backend::CrosstermBackend,
};

fn main() {
	let terminal = Terminal::new(CrosstermBackend::new(io::sdout())).unwrap();
}