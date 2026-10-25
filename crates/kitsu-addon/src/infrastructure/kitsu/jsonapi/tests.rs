use serde_json::{Value, json};

use super::*;

fn deserialised(document: Value) -> String {
    deserialise(js_value(document)).unwrap().to_string()
}

#[test]
fn missing_include_resolves_to_its_identifier() {
    let document = json!({
        "data": { "id": "1", "type": "anime", "attributes": { "a": 1 },
            "relationships": { "genres": { "data": [{ "type": "genres", "id": "9" }] } } },
        "included": []
    });
    assert_eq!(
        deserialised(document),
        r#"{"data":{"id":"1","type":"anime","genres":{"data":[{"id":"9","type":"genres"}]},"a":1}}"#
    );
}

#[test]
fn relationship_without_data_keeps_links_and_meta() {
    let document = json!({
        "data": { "id": "1", "type": "anime",
            "relationships": { "castings": { "links": { "self": "s" }, "meta": { "count": 2 } } },
            "attributes": { "a": 1 } }
    });
    assert_eq!(
        deserialised(document),
        r#"{"data":{"id":"1","type":"anime","castings":{"links":{"self":"s"},"meta":{"count":2}},"a":1}}"#
    );
}

#[test]
fn to_one_relationship_puts_data_before_links_and_meta() {
    let document = json!({
        "data": { "id": "1", "type": "anime",
            "relationships": { "media": { "links": { "self": "s" }, "meta": { "m": 1 }, "data": { "type": "anime", "id": "2" } } } },
        "included": [{ "id": "2", "type": "anime", "attributes": { "b": 2 } }]
    });
    assert_eq!(
        deserialised(document),
        r#"{"data":{"id":"1","type":"anime","media":{"data":{"id":"2","type":"anime","b":2},"links":{"self":"s"},"meta":{"m":1}}}}"#
    );
}

#[test]
fn reused_include_takes_the_meta_of_each_reference() {
    let document = json!({
        "data": [
            { "id": "1", "type": "anime", "relationships": { "genres": { "data": [{ "type": "genres", "id": "9", "meta": { "r": 1 } }] } } },
            { "id": "2", "type": "anime", "relationships": { "genres": { "data": [{ "type": "genres", "id": "9" }] } } }
        ],
        "included": [{ "id": "9", "type": "genres", "attributes": { "name": "Action" } }]
    });
    assert_eq!(
        deserialised(document),
        concat!(
            r#"{"data":[{"id":"1","type":"anime","genres":{"data":[{"id":"9","type":"genres","meta":{"r":1},"name":"Action"}]}},"#,
            r#"{"id":"2","type":"anime","genres":{"data":[{"id":"9","type":"genres","name":"Action"}]}}]}"#
        )
    );
}

#[test]
fn later_list_item_resolves_from_the_raw_list() {
    let document = json!({
        "data": [
            { "id": "1", "type": "anime", "attributes": { "t": "one" }, "relationships": { "sequel": { "data": { "type": "anime", "id": "2" } } } },
            { "id": "2", "type": "anime", "attributes": { "t": "two" }, "relationships": { "genres": { "data": [{ "type": "genres", "id": "9" }] } } }
        ],
        "included": [{ "id": "9", "type": "genres", "attributes": { "name": "Action" } }]
    });
    assert_eq!(
        deserialised(document),
        concat!(
            r#"{"data":[{"id":"1","type":"anime","sequel":{"data":{"id":"2","type":"anime","genres":{"data":[{"id":"9","type":"genres","name":"Action"}]},"t":"two"}},"t":"one"},"#,
            r#"{"id":"2","type":"anime","genres":{"data":[{"id":"9","type":"genres","name":"Action"}]},"t":"two"}]}"#
        )
    );
}

#[test]
fn earlier_list_item_resolves_to_its_deserialised_form() {
    let document = json!({
        "data": [
            { "id": "1", "type": "anime", "attributes": { "t": "one" }, "relationships": { "genres": { "data": [{ "type": "genres", "id": "9" }] } } },
            { "id": "2", "type": "anime", "attributes": { "t": "two" }, "relationships": { "prequel": { "data": { "type": "anime", "id": "1" } } } }
        ],
        "included": [{ "id": "9", "type": "genres", "attributes": { "name": "Action" } }]
    });
    assert_eq!(
        deserialised(document),
        concat!(
            r#"{"data":[{"id":"1","type":"anime","genres":{"data":[{"id":"9","type":"genres","name":"Action"}]},"t":"one"},"#,
            r#"{"id":"2","type":"anime","prequel":{"data":{"id":"1","type":"anime","genres":{"data":[{"id":"9","type":"genres","name":"Action"}]},"t":"one"}},"t":"two"}]}"#
        )
    );
}

#[test]
fn cyclic_list_references_terminate() {
    let document = json!({
        "data": [
            { "id": "1", "type": "anime", "relationships": { "sequel": { "data": { "type": "anime", "id": "2" } } } },
            { "id": "2", "type": "anime", "relationships": { "prequel": { "data": { "type": "anime", "id": "1" } } } }
        ]
    });
    assert!(deserialise(document).is_ok());
}

#[test]
fn null_relationship_fails() {
    let document = json!({ "data": { "id": "1", "type": "anime", "relationships": { "x": null } } });
    assert!(deserialise(document).is_err());
}

#[test]
fn included_without_data_fails() {
    assert!(deserialise(json!({ "included": [{ "id": "1", "type": "a" }] })).is_err());
}

#[test]
fn parse_orders_integer_keys_first_and_drops_float_zero_fractions() {
    let document = parse(br#"{"data":{"id":"1","type":"anime","attributes":{"ratingFrequencies":{"b":"x","10":"1","2":"3"},"rating":1.0}}}"#).unwrap();
    assert_eq!(
        deserialise(document).unwrap().to_string(),
        r#"{"data":{"id":"1","type":"anime","ratingFrequencies":{"2":"3","10":"1","b":"x"},"rating":1}}"#
    );
}

#[test]
fn query_skips_nothing_for_empty_objects() {
    let params = json!({ "filter": {}, "a": 1 });
    assert_eq!(query(params.as_object().unwrap()), "&a=1");
}

#[test]
fn query_encodes_like_encode_uri_component() {
    let params = json!({ "filter": { "text": "Fate/stay night: ü & co", "ids": [1, 2] }, "sort": "-x", "n": null });
    assert_eq!(
        query(params.as_object().unwrap()),
        "filter%5Btext%5D=Fate%2Fstay%20night%3A%20%C3%BC%20%26%20co&filter%5Bids%5D=1&filter%5Bids%5D=2&sort=-x&n=null"
    );
}

#[test]
fn encode_uri_component_keeps_its_unreserved_marks() {
    assert_eq!(encode_uri_component("a-_.!~*'()b"), "a-_.!~*'()b");
}
