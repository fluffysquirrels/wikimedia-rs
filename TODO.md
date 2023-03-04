# To do

## Must do before publishing

* Handle it gracefully in the cronjob when:
    *  The status of the job is not "done" (e.g. still in
       progress). At the moment the `download` subcommand just returns
       an Err() with a message, which is really machine readable. Probably
       return a custom `Error` struct with an `kind: ErrorKind` field.
    *  Downloads fail. Retry automatically after a short delay or next
       time the cronjob runs.
* Download files with a temporary extension, then move them into place when done.
* Support logging as JSON.
* `--mirror-url` argument for `download` subcommand.


## Might do

* Subcommand to list dumps
* Subcommand to list versions
* Subcommand to list jobs
* Subcommand to list files for a job
* More performant, elegant, featureful async streaming downloads.
    * Progress bar
    * Write while beginning next read.
    * Configurable timeout
    * Cancellation support
* Specify a version to `download` subcommand with `--version`, still pick the latest by default.
* Avoid boilerplate to record context variables in `download` subcommand.
    * Perhaps use `tracing::span` to record context variables, with
      events setting their parent to that span
* Unit tests for file relative URL validation
* Validate dump name, job name to have no relative paths, path traversal.
