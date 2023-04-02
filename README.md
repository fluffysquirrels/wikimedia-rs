# `wikimedia` crates (including `wmd` CLI tool)

Open source Rust libraries and tools for downloading and viewing data
from [Wikimedia Foundation][wikimedia], the non-profit behind
Wikipedia and other projects.

There are 3 related crates in the [`wikimedia-rs` source repository][repo]:

* `wikimedia`: library to download and parse data from Wikimedia.  
  [Crate](https://crates.io/crates/wikimedia) |
  [Documentation](https://docs.rs/wikimedia)
* `wikimedia-store`: library to store MediaWiki pages, supporting
  search and import from Wikimedia dump files.  
  [Crate](https://crates.io/crates/wikimedia-store) |
  [Documentation](https://docs.rs/wikimedia-store)
* `wikimedia-download`: CLI tool `wmd` to download data from Wikimedia
  and view it over a web interface.  
  [Crate](https://crates.io/crates/wikimedia-download)

## To install

Install the Rust toolchain. [`rustup`](https://rustup.rs/) is the
standard tool to do this.

These crates are primarily developed and tested on Linux. They are
tested occasionally on macOS.  The utility scripts in `bin/` are
written in `bash`, and all further instructions will assume you are
running on a Unix-based machine.

The dependencies selected are cross-platform so hopefully would work
on a Windows machine, but this hasn't been tested at all. If you want
to run on Windows, try using WSL or Cygwin. Pull requests welcome if
you want to add support!

Run this command to build and install `wmd` with
Rust's package manager `cargo`:

```sh
RUSTFLAGS="--cfg tracing_unstable" cargo +nightly install wikimedia-download
```

By default this will install `wmd` to `~/.cargo/bin/wmd`, make sure this is on
your shell's path.

Viewing the downloaded pages requires `pandoc` on your executable path
to convert the MediaWiki Wikitext markup to HTML.
See their [releases download page](https://github.com/jgm/pandoc/releases),
and their [installation instructions page](https://pandoc.org/installing.html).

## Quick start

To show help instructions for `wmd`'s subcommands:
```sh
# Help for `wmd` and its list of subcommands
wmd help

# For help for the subcommand `download` use one of these:
wmd help download
wmd download --help
```

To download the text for the latest version of all articles on English
Wikipedia (about 20 GB to download as of 2023-03-20):

```sh
export WMD_MIRROR_URL=https://ftp.acc.umu.se/mirror/wikimedia.org/dumps

wmd download  --dump enwiki \
              --version latest \
              --job articlesdump
```

Example download finished message:

```
Downloading job files complete
|   download_dir = /home/alex/wm/out/dumps/enwiki/20230320/articlesdump
|   dump         = enwiki
|   version      = 20230320
|   job          = articlesdump
```

By default the files will be downloaded to
* `~/.local/share/wmd` on Linux
* `~/Library/Application Support/wmd` on macOS
* `C:\Users\%USERNAME%\AppData\Local\wmd` on Windows

This can be overriden with the environment variable `WMD_OUT_DIR` or
CLI argument `--out-dir`; see `wmd help download` for more
information.

If some files have already been downloaded, their checksums will be
verified and if correct they will not be downloaded again.

[Wikimedia's main data dump download server](https://dumps.wikimedia.org/)
limits the number of network connections and download rate, so it's recommended to choose a dump mirror to download from. Official mirrors are listed on these pages: 
[1](https://meta.wikimedia.org/wiki/Mirroring_Wikimedia_project_XML_dumps#Current_mirrors) 
[2](https://dumps.wikimedia.org/mirrors.html).
The example in the script above is the one I've been using to test `wmd`; it's located in Sweden, which is geographically close to me.

To easily retrieve the articles they must be imported into `wmd`'s store:

```sh
wmd import-dump   --dump enwiki \
                  --version 20230320 \
                  --job articlesdump
```

Use the same dump and job you downloaded earlier, and the version that `wmd download` reported.
An import of the latest version of all articles on English Wikipedia will occupy about 80 GB of disk storage. This is larger than the download size because the store is currently not compressed, but this is planned.

Once the import command is done, you can view the downloaded pages in the web interface:

```sh
wmd web
```

Example output:

```
  2023-04-02T22:25:08.86645692Z  INFO wmd::commands::web: Listening on http, url: http://localhost:8089/
    at crates/wikimedia-download/src/commands/web.rs:89 on ThreadId(1)
```

Visit the URL in the log message: [`http://localhost:8089`](http://localhost:8089).

Set the environment varible `RUST_LOG` to configure logging levels and filtering. This application uses the `tracing-subscriber` crate for logging, see [their documentation for the available logging configuration directives][log-directives]. Note that many of these directives can be supplied separated by commas.

## Shell completion setup

The currently supported shells are: bash, elvish, fish, powershell, and zsh.

Save a completion file for your shell with `wmd completion`, for example for zsh:

```sh
wmd completion --shell zsh > completion.zsh
```

Then follow your shell's instructions to load the file.

`wmd`'s argument parsing is implemented with the [`clap` crate](https://crates.io/crates/clap),
and shell completion files are generated with the
[`clap_complete` crate](https://crates.io/crates/clap_complete).

## Development instructions

* Clone the source code with git:  
  `git clone https://github.com/fluffysquirrels/wikimedia-rs.git`
* Build with the script `bin/build`. This builds in release mode,
  which is strongly recommended. In debug mode without optimisations
  some of the data processing will be very slow.
* Run tests with `bin/test`.
* Build then run with `bin/wmd`.  
  I recommend adding a symlink to this file on your path. On my machine `~/bin`
  is on my path, so I just needed to run `ln -s ${PWD}/bin/wmd ~/bin`.

Store pages are encoded using [Cap'n Proto](https://capnproto.org/)'s
Rust implementation [`capnproto-rust`](https://github.com/capnproto/capnproto-rust).
This requires generating accessor code (currently in [`crates/wikimedia-store/capnp/generated`][capnp-gen])
from `.capnp` schema files (currently in [`crates/wikimedia-store/capnp/`][capnp-schema]).

The accessor code is checked into the source code repository so the
generator tools do not need to be installed to build the code when
using `cargo install` or the build scripts, but if the schema files
are modified the build scripts will detect this and regenerate the
accessor code automatically.

You must install these dependencies on your executable path to regenerate the accessor code:
* `capnp`, the Cap'n Proto schema compiler;
  see [the install instructions](https://capnproto.org/install.html).
* `capnpc-rust`, the `capnp` Rust plugin. Install it with `cargo install capnpc`.

[log-directives]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/struct.EnvFilter.html#directives
[repo]: https://github.com/fluffysquirrels/wikimedia-rs
[wikimedia]: https://www.wikimedia.org/
[capnp-gen]: https://github.com/fluffysquirrels/wikimedia-rs/tree/2dbed585efd57262d2e3bced91b4671be3aca0f2/crates/wikimedia-store/capnp/generated
[capnp-schema]: https://github.com/fluffysquirrels/wikimedia-rs/tree/2dbed585efd57262d2e3bced91b4671be3aca0f2/crates/wikimedia-store/capnp
