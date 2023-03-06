use crate::{
    args::{CommonArgs, DumpNameArg, JsonOutputArg, VersionSpecArg},
    http,
    operations,
    Result,
    types::{JobOutput, JobStatus},
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
    job_name: Option<String>,

    #[clap(flatten)]
    json: JsonOutputArg,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let dump_name = &*args.dump_name.value;

    let client = http::client()?;

    let(ver, ver_status) = operations::get_dump_version_status(&client, &args.dump_name,
                                                               &args.version.value).await?;

    let jobs: Vec<(String, JobStatus)> = match args.job_name.as_ref() {
        Some(job_name) => {
            let Some(job_status) = ver_status.jobs.get(job_name) else {
                return Err(anyhow::Error::msg(format!(
                    "No status found for job job_name='{job_name}' version='{ver}' \
                     dump_name='{dump_name}'",
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

    if args.json.value {
        for (job_name, job_status) in jobs.iter() {
            let job = JobOutput {
                name: job_name.clone(),
                files_size: job_status.files.values()
                                            .map(|file_info| file_info.size)
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
