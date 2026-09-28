use extrema_infra::prelude::{InfraError, InfraResult};

/// The single element of a list payload; none or several is an error.
pub fn exactly_one<T>(items: Vec<T>) -> InfraResult<T> {
    let mut items = items.into_iter();
    match (items.next(), items.next()) {
        (Some(item), None) => Ok(item),
        (None, _) => Err(InfraError::ApiCliError(
            "expected exactly one item, got none".to_string(),
        )),
        (Some(_), Some(_)) => Err(InfraError::ApiCliError(
            "expected exactly one item, got several".to_string(),
        )),
    }
}

/// Only the first instrument is used; venues here take one market per subscription message.
#[cfg(any(
    feature = "arcus",
    feature = "edgex",
    feature = "lighter",
    feature = "nado",
    feature = "pacifica"
))]
pub(crate) fn single_ws_inst<'a>(venue: &str, insts: Option<&'a [String]>) -> InfraResult<&'a str> {
    let insts = insts.unwrap_or_default();
    let Some(inst) = insts.first() else {
        return Err(InfraError::ApiCliError(format!(
            "{venue} ws requires at least one instrument"
        )));
    };

    if insts.len() > 1 {
        tracing::warn!(
            "{venue} ws supports one instrument per subscription message; got {} instruments: {:?}",
            insts.len(),
            insts
        );
    }

    Ok(inst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_rejects_empty_and_several() {
        assert_eq!(exactly_one(vec![5u8]).unwrap(), 5);
        assert!(exactly_one(Vec::<u8>::new()).is_err());
        assert!(exactly_one(vec![1u8, 2]).is_err());
    }

    #[cfg(any(
        feature = "arcus",
        feature = "edgex",
        feature = "lighter",
        feature = "nado",
        feature = "pacifica"
    ))]
    #[test]
    fn single_ws_inst_takes_the_first_instrument() {
        let insts = vec!["@1".to_string(), "@2".to_string()];

        assert_eq!(single_ws_inst("Lighter", Some(&insts)).unwrap(), "@1");
        assert!(single_ws_inst("Lighter", Some(&[])).is_err());
        assert!(single_ws_inst("Lighter", None).is_err());
    }
}
