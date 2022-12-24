use crate::{codegen::BookServiceInterface, MyExampleContext};

use super::api::Book;

pub struct BookService {}

impl BookServiceInterface for BookService {
    fn get_book(
        &self,
        request: super::api::GetBookRequest,
        ctx: &MyExampleContext,
    ) -> super::api::Book {
        assert_eq!(ctx.hardcoded_database.len(), 2);

        let book = ctx
            .hardcoded_database
            .iter()
            .find(|book_record| book_record.isbn == request.isbn);

        book.map(Book::clone).unwrap_or_default()
    }
}
