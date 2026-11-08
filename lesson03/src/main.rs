mod errors;
mod repository;

use errors::RepositoryError;
use repository::{FileUserRepository, welcome};
use std::io;

fn error_kind(err: &RepositoryError) -> &'static str {
    match err {
        RepositoryError::Read(source) if source.kind() == io::ErrorKind::NotFound => "missing-file",
        RepositoryError::Decode(source) if source.is_syntax() => "invalid-json",
        _ => "repository-error",
    }
}

fn main() {
    let cases = [
        ("found", "data/users.json", 1),
        ("absent", "data/users.json", 404),
        ("missing-file", "data/missing.json", 1),
        ("invalid-json", "data/broken.json", 1),
        ("found-again", "data/users.json", 2),
    ];

    for (name, path, id) in cases {
        let repo = FileUserRepository {
            path: String::from(path),
        };
        match welcome(&repo, id) {
            Ok(message) => println!("case={name} result={message}"),
            Err(err) => {
                println!("case={name} error={}", error_kind(&err));
                println!("  detail: path={path}; id={id}; {err}");
            }
        }
    }
}
