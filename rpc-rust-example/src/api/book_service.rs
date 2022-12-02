use protobuf::Message;

use crate::codegen::BookServiceInterface;

use super::api::Book;

pub struct BookService {}

impl BookServiceInterface for BookService {
    fn get_book(&self, request: super::api::GetBookRequest) -> super::api::Book {
        let mut book = Book::default();
        book.set_author("YO".to_string());
        book.set_isbn(100);
        book.set_title("CS".to_string());
        book
    }
}
