# Contributing to Strata

Thanks for helping improve Strata. Bug reports, feature suggestions, docs, and code contributions are welcome.

## Before you start

- Read the [code of conduct](CODE_OF_CONDUCT.md).
- Search [existing issues](https://github.com/fikrifirmanf/strata/issues) before opening a new one.
- For a large feature or a new database driver, open an issue to discuss the scope first.
- Use sample data and disposable databases while testing.

## Report a bug

Include the Strata version, operating system, database engine and version, steps to reproduce, and expected and actual behavior. Screenshots and a minimal query or schema help. Remove passwords, tokens, connection URLs, and private data.

For security concerns, email [dev@fikrifirmanf.com](mailto:dev@fikrifirmanf.com) privately instead of filing a public issue.

## Send a pull request

1. Fork the repository and create a branch from `main`.
2. Follow the [development setup](docs/DEVELOPMENT.md).
3. Make a focused change and update any affected documentation.
4. Run `cargo test --locked` and `cargo build --locked`. Add a regression test for a behavior change where practical.
5. Open a pull request explaining the problem, the resulting behavior, and how you tested it. Include before and after screenshots for visual changes.

Follow the style of the surrounding code. Avoid unrelated formatting changes. Database changes should account for engine differences, read-only connections, cancellation, and transaction boundaries. Live integration tests need disposable servers; see the development guide.

## Licensing

Contributions are accepted under the project's [MIT License](LICENSE). Keep third-party copyright and license notices with any assets or code you add, and update [third-party notices](THIRD_PARTY_NOTICES.md) when needed.
