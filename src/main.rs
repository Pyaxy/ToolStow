//! ToolStow binary entrypoint.

use clap::Parser;
use toolstow::config::Config;

fn main() {
    let _config = Config::parse();
}
