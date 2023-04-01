use anyhow::bail;
use crate::args::{CommonArgs, DumpNameArg, JsonOutputArg, VersionSpecArg};
use wikimedia::{
    dump::{self, JobName, JobOutput, JobStatus},
    http,
    Result,
};

/// Get data about a dump version's jobs.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(flatten)]
    dump_name: DumpNameArg,

    #[clap(flatten)]
    version: VersionSpecArg,

    /// The specific job name to get. By default information is returned about all jobs in the dump version.
    #[arg(long = "job")]
    job_name: Option<JobName>,

    #[clap(flatten)]
    json: JsonOutputArg,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let dump_name = &args.dump_name.value;
    let version_spec = &args.version.value;

    let client = http::metadata_client(&args.common.http_options()?.build()?)?;

    let(version, version_status) = dump::download::get_dump_version_status(&client, dump_name,
                                                                           version_spec).await?;

    let mut jobs: Vec<(String, JobStatus)> = match args.job_name.as_ref() {
        Some(job_name) => {
            let Some(job_status) = version_status.jobs.get(&*job_name.0) else {
                bail!("No status found for job job_name='{job_name}' version='{version}' \
                       dump_name='{dump_name}'",
                      dump_name = dump_name.0,
                      job_name = job_name.0,
                      version = version.0);
            };
            vec![(job_name.0.clone(), job_status.clone())]
        },
        None => {
            version_status.jobs.iter()
                               .map(|(k, v)| (k.clone(), v.clone()))
                               .collect::<Vec<(String, JobStatus)>>()
        }
    };
    jobs.sort_by(|(name1, _), (name2, _)| name1.as_str().cmp(name2.as_str()));

    if args.json.value {
        for (job_name, job_status) in jobs.iter() {
            let job = JobOutput {
                name: job_name.clone(),
                files_size: job_status.files.values()
                                            .map(|file_info| file_info.size.unwrap_or(0))
                                            .sum(),
                files_count: job_status.files.len(),
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
