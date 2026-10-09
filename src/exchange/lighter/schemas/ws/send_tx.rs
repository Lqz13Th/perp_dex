use serde::{Deserialize, Deserializer};

/// Reply to `jsonapi/sendtx` / `jsonapi/sendtxbatch` on a [`LIGHTER_TX_CHANNEL`] task, read from
/// `WsOtherMessage::raw_json`; control frames (`connected`, `pong`) do not parse.
///
/// [`LIGHTER_TX_CHANNEL`]: crate::exchange::lighter::config_assets::LIGHTER_TX_CHANNEL
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum WsSendTxLighter {
    Sent {
        #[serde(default)]
        id: Option<String>,
        /// One hash per transaction, in order.
        #[serde(deserialize_with = "one_or_many")]
        tx_hash: Vec<String>,
        #[serde(default)]
        predicted_execution_time_ms: i64,
    },
    Rejected {
        #[serde(default)]
        id: Option<String>,
        error: WsSendTxErrorLighter,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct WsSendTxErrorLighter {
    pub code: i64,
    pub message: String,
}

fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(h) => vec![h],
        OneOrMany::Many(h) => h,
    })
}

impl WsSendTxLighter {
    pub fn id(&self) -> Option<&str> {
        match self {
            WsSendTxLighter::Sent { id, .. } | WsSendTxLighter::Rejected { id, .. } => {
                id.as_deref()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_and_rejections_parse() {
        let one: WsSendTxLighter = serde_json::from_str(
            r#"{"code":200,"id":"probe-b","predicted_execution_time_ms":1791528509552,"tx_hash":"3a54","type":"jsonapi/sendtx"}"#,
        )
        .unwrap();
        assert!(matches!(&one, WsSendTxLighter::Sent { tx_hash, .. } if tx_hash == &["3a54"]));
        assert_eq!(one.id(), Some("probe-b"));

        let batch: WsSendTxLighter = serde_json::from_str(
            r#"{"code":200,"id":"b2","predicted_execution_time_ms":1,"tx_hash":["73c5","8a01"],"type":"jsonapi/sendtxbatch"}"#,
        )
        .unwrap();
        assert!(matches!(&batch, WsSendTxLighter::Sent { tx_hash, .. } if tx_hash.len() == 2));

        let bad: WsSendTxLighter = serde_json::from_str(
            r#"{"error":{"code":21104,"message":"invalid nonce"},"id":"probe-c"}"#,
        )
        .unwrap();
        assert!(matches!(&bad, WsSendTxLighter::Rejected { error, .. } if error.code == 21104));
        assert_eq!(bad.id(), Some("probe-c"));
        let anonymous: WsSendTxLighter =
            serde_json::from_str(r#"{"error":{"code":21501,"message":"invalid tx info"}}"#)
                .unwrap();
        assert_eq!(anonymous.id(), None);
    }

    #[test]
    fn control_frames_are_not_replies() {
        for frame in [
            r#"{"session_id":"a1df","type":"connected"}"#,
            r#"{"type":"pong"}"#,
        ] {
            assert!(
                serde_json::from_str::<WsSendTxLighter>(frame).is_err(),
                "{frame}"
            );
        }
    }
}
