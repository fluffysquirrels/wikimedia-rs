// use anyhow::Context;
use crate::{
    args::{CommonArgs, DumpNameArg},
    http,
    operations,
    Result,
    types::{JobOutput, JobStatus},
};

/// Get a list of dump jobs
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(flatten)]
    dump_name: DumpNameArg,

    /// The specific job name to get. By default information is returned about all jobs in the dump version.
    #[arg(long = "job")]
    job_name: Option<String>,

    /// Print results to stdout as JSON. By default the job names will be printed as text.
    #[arg(long, default_value_t = false)]
    json: bool
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let dump_name = &*args.dump_name.value;

    let client = http::client()?;

    let mut vers = operations::get_dump_versions(&client, &args.dump_name).await?;
    if vers.is_empty() {
        return Err(anyhow::Error::msg(format!("No versions found for dump {dump_name}")));
    }
    vers.sort();
    // Re-bind as immutable.
    let vers = vers;

    let ver = vers.last().expect("vers not empty");

    let ver_status = operations::get_dump_version_status(&client, &args.dump_name, &ver).await?;

    let jobs: Vec<(String, JobStatus)> = match args.job_name.as_ref() {
        Some(job_name) => {
            let Some(job_status) = ver_status.jobs.get(job_name) else {
                return Err(anyhow::Error::msg(format!("No status found for job job_name={job_name} version={ver} dump_name={dump_name}",
                                                      ver = ver.0)));
            };
            vec![(job_name.clone(), job_status.clone())]
        },
        None => {
            ver_status.jobs.iter()
                           .map(|(k, v)| (k.clone(), v.clone()))
                           .collect::<Vec<(String, JobStatus)>>()
        }
    };

    if args.json {
        for (job_name, job_status) in jobs.iter() {
            let job = JobOutput {
                name: job_name.clone(),
                status: job_status.clone(),
            };
            serde_json::to_writer_pretty(&std::io::stdout(), &job)?;
            println!();
        }
    } else {
        // json == false, so print job names only
        for (job_name, _) in jobs.iter() {
            println!("{}", job_name);
        }
    }

    Ok(())
}
