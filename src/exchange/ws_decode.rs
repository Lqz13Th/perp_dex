use serde::de::DeserializeOwned;

/// Tries the runner's expected data shape before falling back to the full frame.
#[cfg_attr(
    not(any(
        feature = "lighter",
        feature = "aster",
        feature = "arcus",
        feature = "apex",
        feature = "edgex",
        feature = "extended",
        feature = "grvt",
        feature = "nado",
        feature = "pacifica"
    )),
    allow(dead_code)
)]
pub(crate) fn decode_preferred<Frame, Preferred, Wrap>(
    frame: &[u8],
    wrap: Wrap,
) -> serde_json::Result<Frame>
where
    Frame: DeserializeOwned,
    Preferred: DeserializeOwned,
    Wrap: FnOnce(Preferred) -> Frame,
{
    match serde_json::from_slice::<Preferred>(frame) {
        Ok(message) => Ok(wrap(message)),
        Err(error) => {
            tracing::trace!(
                preferred = std::any::type_name::<Preferred>(),
                error = %error,
                "WS preferred decoder fallback"
            );
            serde_json::from_slice(frame)
        },
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct TestData {
        value: u64,
    }

    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum TestFrame {
        Data(TestData),
        Event { id: u64 },
    }

    #[test]
    fn prefers_the_expected_shape() {
        let frame =
            decode_preferred::<TestFrame, TestData, _>(br#"{"value":7}"#, TestFrame::Data).unwrap();

        assert!(matches!(frame, TestFrame::Data(TestData { value: 7 })));
    }

    #[test]
    fn falls_back_to_the_full_frame() {
        let frame =
            decode_preferred::<TestFrame, TestData, _>(br#"{"id":3}"#, TestFrame::Data).unwrap();

        assert!(matches!(frame, TestFrame::Event { id: 3 }));
    }

    #[test]
    fn returns_the_full_frame_error_when_nothing_matches() {
        assert!(
            decode_preferred::<TestFrame, TestData, _>(br#"{"other":1}"#, TestFrame::Data).is_err()
        );
        assert!(
            decode_preferred::<TestFrame, TestData, _>(br#"{"value":"#, TestFrame::Data).is_err()
        );
    }
}
