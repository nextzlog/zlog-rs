use std::io;
use std::sync::Arc;
use std::time::Duration;

use haml::remote::{Conn, Listener};
use zlog_tls::{LineConn, Server};

pub struct Bridge(pub LineConn);

impl Conn for Bridge {
    fn peer(&self) -> String {
        let p = self.0.peer();
        match p.name.is_empty() {
            true => p.addr.to_string(),
            false => format!("{} ({})", p.name, p.addr),
        }
    }

    fn read_line(&mut self, wait: Duration) -> io::Result<Option<String>> {
        Ok(self
            .0
            .read_line(wait)?
            .map(|l| String::from_utf8_lossy(&l).into_owned()))
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.0.write_line(line.as_bytes())
    }

    fn shutdown(&mut self) {
        self.0.shutdown();
    }
}

pub struct Accept(pub Arc<Server>);

impl Listener for Accept {
    fn accept(&mut self, wait: Duration) -> io::Result<Option<Box<dyn Conn>>> {
        Ok(self
            .0
            .accept(wait)
            .map(|c| Box::new(Bridge(c)) as Box<dyn Conn>))
    }
}
