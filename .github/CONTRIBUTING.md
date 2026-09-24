# Contributing

Thank you for looking at Statup. Issues and pull requests are welcome; small,
focused changes are the easiest to review.

## Before you start

Open an issue for anything beyond a fix, so the direction is agreed before
the work. Statup aims to stay a single binary with an embedded database and
no runtime dependency; a change that adds a service, a framework or a
third-party call at runtime needs a strong case.

## Setting up

Rust 1.88 or newer and the Tailwind CSS standalone CLI, as described under
Development in the README. Then:

```bash
./scripts/build-css.sh
cargo run
```

## Checks

Every pull request must pass:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Add or adjust tests with the change. Integration tests in `tests/` run the
real router over TCP; unit tests sit next to the code they cover.

## Conventions

- Code, comments, commit messages and documentation are in English. The
  interface is translated in `locales/fr.json` and `locales/en.json`, which
  hold the same keys in the same order; a test checks that every key used by
  the code exists in both.
- No em dash, en dash or double hyphen in prose. Use a comma, a colon or a
  full stop.
- No inline script, event handler or style in a template: the Content
  Security Policy forbids them. Behaviour lives in `static/js`, presentation
  in `static/css`.
- Colours and sizes go through the tokens in `static/css/tokens.css`. A
  status is a coloured dot next to a neutral word, never a coloured word.
- Errors are returned, never unwrapped, outside tests.
- Commit messages are short and say what the change does, in the imperative
  or as a plain statement.

## License

By contributing you agree that your work is released under the AGPL-3.0-or-later
license of the project.
