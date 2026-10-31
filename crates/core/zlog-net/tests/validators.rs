use zlog_net::Validators;

#[test]
fn validators_round_trip_as_json() {
    let v = Validators {
        etag: Some("\"abc\"".into()),
        last_modified: Some("Sat, 24 Oct 2026 00:00:00 GMT".into()),
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"etag":"\"abc\"","last_modified":"Sat, 24 Oct 2026 00:00:00 GMT"}"#
    );
    assert_eq!(serde_json::from_str::<Validators>(&json).unwrap(), v);
}

#[test]
fn missing_and_null_fields_are_none() {
    let v: Validators = serde_json::from_str(r#"{"etag":null}"#).unwrap();
    assert_eq!(v, Validators::default());
}
