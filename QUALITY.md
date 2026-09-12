# Quality checks

[AGENTS.md](AGENTS.md) defines the project baseline. Fast checks are:

```sh
make lint static-analysis
```

The native Debian 13 pull-request gate runs Doxygen once, then build, tests,
staged installation, Debian packaging, autopkgtest, and archive checks on
amd64 and arm64.
Production Rust code requires 100% line and branch coverage on amd64 only.
`make ci` includes that production-coverage check. Staged installation,
Debian package verification and autopkgtest compile and run the public
descriptor smoke test through installed pkg-config metadata rather than using
the build-tree header or library. Doxygen generation checks documented
production Rust declarations and embeds their complete source context in the
published reference pages.
The `container-coverage` target builds a disposable derived image, pulls its
maintained base first, and runs coverage through the labeled launcher.

The project owns no persistent test containers. The launcher removes only
containers carrying its exact project and workspace labels before and after a
run.
