use zlog_core::{Book, Command, Log, Qso, Registry};
use zlog_general::GeneralRules;
use zlog_prefix::PrefixExtension;

#[test]
fn entity_is_written_back_once() {
    let (mut book, _) = Book::open(
        Log::default(),
        Box::new(GeneralRules),
        "open",
        Registry::new().with(PrefixExtension),
    );
    let messages = book
        .execute(Command::Insert(Qso {
            callsign: "K1ABC".into(),
            ..Qso::default()
        }))
        .unwrap();
    assert_eq!(book.log().qsos.len(), 1);
    assert_eq!(book.log().qsos[0].entity, "US");
    assert_eq!(book.log().qsos[0].reserve3 % 100, 1, "edited exactly once");
    assert_eq!(messages, vec!["K1ABC: US"]);
}
