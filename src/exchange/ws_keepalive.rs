use std::time::{Duration, Instant};

use extrema_infra::prelude::{AckHandle, CommandHandle, InfraResult, TaskCommand};

/// Some venues close a connection that has not sent a client frame for a while,
/// however busy the stream is, and infra only pings after ten silent seconds.
///
/// Keep one per task and call [`on_frame`](Self::on_frame) from its `on_lob` /
/// `on_trade`: pinging only while frames arrive keeps the ping out of the command
/// queue during a reconnect, where infra expects `WsConnect` first.
#[derive(Clone, Debug)]
pub struct WsKeepalive {
    msg: fn() -> String,
    interval: Duration,
    last_sent: Option<Instant>,
}

impl WsKeepalive {
    /// `msg` builds the frame each time one is due, so it can carry the current time.
    pub fn new(msg: fn() -> String, interval: Duration) -> Self {
        Self {
            msg,
            interval,
            last_sent: None,
        }
    }

    /// Whether a ping is due at `now`; marks it sent when it is.
    pub fn due(&mut self, now: Instant) -> bool {
        match self.last_sent {
            Some(sent) if now.duration_since(sent) < self.interval => false,
            _ => {
                self.last_sent = Some(now);
                true
            },
        }
    }

    pub async fn on_frame(&mut self, handle: &CommandHandle) -> InfraResult<()> {
        if !self.due(Instant::now()) {
            return Ok(());
        }

        handle
            .send_command(
                TaskCommand::WsMessage {
                    msg: (self.msg)(),
                    ack: AckHandle::none(),
                },
                None,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_frame_pings_then_once_per_interval() {
        let mut keepalive = WsKeepalive::new(|| "ping".into(), Duration::from_secs(30));
        let start = Instant::now();

        assert!(keepalive.due(start));
        assert!(!keepalive.due(start + Duration::from_secs(1)));
        assert!(!keepalive.due(start + Duration::from_millis(29_999)));
        assert!(keepalive.due(start + Duration::from_secs(30)));
        assert!(!keepalive.due(start + Duration::from_secs(45)));
        assert!(keepalive.due(start + Duration::from_secs(61)));
    }
}
