use crate::{codegen::BookServiceInterface, MyExampleContext};

use super::api::Book;

pub struct BookService {}

impl BookServiceInterface for BookService {
    fn get_book(
        &self,
        request: super::api::GetBookRequest,
        ctx: &MyExampleContext,
    ) -> super::api::Book {
        assert_eq!(ctx.hardcoded_database.len(), 1);

        let book = ctx
            .hardcoded_database
            .iter()
            .find(|book_record| book_record.isbn == request.isbn);

        let mut final_book = Book::default();

        if book.is_some() {
            let b = book.unwrap();
            final_book.set_author(b.author.clone());
            final_book.set_isbn(b.isbn.clone());
            final_book.set_title(b.title.clone());
            return final_book;
        }

        final_book
    }
}
