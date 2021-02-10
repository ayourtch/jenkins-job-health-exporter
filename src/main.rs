extern crate minreq;
use clap::Clap;
use prometheus_exporter::prometheus::core::{AtomicI64, GenericGauge};
use prometheus_exporter::{self, prometheus::register_counter, prometheus::register_int_gauge};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

#[derive(Debug, Serialize, Deserialize)]
struct OneBuild {
    id: String,
    number: usize,
    result: Option<String>,
    timestamp: usize,
    duration: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct AllBuilds {
    builds: Vec<OneBuild>,
}

#[derive(Debug, Serialize, Deserialize)]
enum MyError {
    GenericError(String),
}

impl From<minreq::Error> for MyError {
    fn from(err: minreq::Error) -> Self {
        MyError::GenericError(format!("Generic error: {:?}", err))
    }
}

fn get_job_builds(opts: &Opts, job: &str) -> Result<AllBuilds, MyError> {
    let host = &opts.jenkins_host;
    let last_builds = opts.last_builds;
    // let url = "https://jenkins.fd.io/job/vpp-verify-master-debian10-x86_64/api/json?tree=builds[number,status,timestamp,id,result]";
    let url = format!(
        "https://{}/job/{}/api/json?tree=builds[number,status,timestamp,id,result,duration]{{,{}}}",
        host, job, last_builds
    );
    let response = minreq::get(url).with_timeout(opts.req_timeout_sec).send()?;
    let result = response.json::<AllBuilds>()?;
    Ok(result)
}

/// This program periodically polls Jenkins jobs that are specified in the parameters,
/// and exports it for Prometheus
#[derive(Clap, Serialize, Deserialize)]
#[clap(version = env!("GIT_VERSION"), author = "Andrew Yourtchenko <ayourtch@gmail.com>")]
struct Opts {
    /// Jenkins hostname to monitor the jobs on
    #[clap(short, long, default_value = "localhost")]
    jenkins_host: String,

    /// Timeout value for the requests, in seconds
    #[clap(long, default_value = "30")]
    req_timeout_sec: u64,

    /// Poll interval - how often to get the job builds status
    #[clap(short, long, default_value = "1800")]
    poll_interval_sec: u64,

    /// Bind Prometheus exporter to this address
    #[clap(short, long, default_value = "127.0.0.1:9186")]
    bind_to: std::net::SocketAddr,

    /// How many "last" builds to look at
    #[clap(short, long, default_value = "10")]
    last_builds: usize,

    /// Jenkins jobs to monitor. If a single element and it is a filename that exists, load all
    /// options from JSON in it. NB: this overrides anything specified on command line.
    // There's a bit of a history to all that: https://github.com/clap-rs/clap/issues/748
    #[clap(required = true)]
    jobs: Vec<String>,
    /// A level of verbosity, and can be used multiple times
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
}

fn calc_metrics(data: &AllBuilds, try_total: usize, verbose: i32) -> (usize, usize, usize, usize) {
    let last_n = data.builds.windows(try_total).nth(0);
    if last_n.is_none() {
        return (0, 0, 0, 0);
    }
    let last_n = last_n.unwrap();
    if verbose > 4 {
        println!("all data: {:#?}", data);
        println!("last data: {:#?}", &last_n);
    }

    let total_count = last_n.len();
    let success_count = last_n
        .iter()
        .filter(|x| {
            if let Some(res) = &x.result {
                res == "SUCCESS"
            } else {
                false
            }
        })
        .count();
    let failure_count = last_n
        .iter()
        .filter(|x| {
            if let Some(res) = &x.result {
                res == "FAILURE"
            } else {
                false
            }
        })
        .count();
    let unstable_count = last_n
        .iter()
        .filter(|x| {
            if let Some(res) = &x.result {
                res == "UNSTABLE"
            } else {
                false
            }
        })
        .count();
    return (success_count, failure_count, unstable_count, total_count);
}

fn main() {
    let opts: Opts = Opts::parse();

    let opts = if let Ok(data) = std::fs::read_to_string(&opts.jobs[0]) {
        let res = serde_json::from_str(&data);
        if res.is_ok() {
            res.unwrap()
        } else {
            serde_yaml::from_str(&data).unwrap()
        }
    } else {
        opts
    };

    if opts.verbose > 4 {
        let data = serde_json::to_string_pretty(&opts).unwrap();
        println!("{}", data);
        println!("===========");
        let data = serde_yaml::to_string(&opts).unwrap();
        println!("{}", data);
    }

    let exporter = prometheus_exporter::start(opts.bind_to.clone()).unwrap();
    println!(
        "Started Prometheus exporter on {}, monitoring {} jobs on {} with {} seconds poll interval",
        &opts.bind_to,
        &opts.jobs.len(),
        &opts.jenkins_host,
        &opts.poll_interval_sec
    );

    let poll_counter =
        register_counter!("poll_cycle_counter", "Number of poll cycles done").unwrap();
    let req_counter = register_counter!(
        "req_counter",
        "Number of total Jenkins API HTTPS requests done"
    )
    .unwrap();
    let req_err_counter = register_counter!(
        "req_err_counter",
        "Number of Jenkins API HTTS requests that ended in error"
    )
    .unwrap();

    let mut gauges_total: HashMap<String, GenericGauge<AtomicI64>> = HashMap::new();
    let mut gauges_success: HashMap<String, GenericGauge<AtomicI64>> = HashMap::new();
    let mut gauges_failure: HashMap<String, GenericGauge<AtomicI64>> = HashMap::new();
    let mut gauges_unstable: HashMap<String, GenericGauge<AtomicI64>> = HashMap::new();
    let mut gauges_reqtime_ms: HashMap<String, GenericGauge<AtomicI64>> = HashMap::new();
    for job in &opts.jobs {
        let gt = register_int_gauge!(
            format!("jenkins_job_total(id=\"{}\")", &job),
            format!("{} last builds total", &job)
        )
        .unwrap();
        let gs = register_int_gauge!(
            format!("jenkins_job_success(id=\"{}\")", &job),
            format!("{} last builds with SUCCESS", &job)
        )
        .unwrap();
        let gf = register_int_gauge!(
            format!("jenkins_job_failure(id=\"{}\")", &job),
            format!("{} last builds with FAILURE", &job)
        )
        .unwrap();
        let gu = register_int_gauge!(
            format!("jenkins_job_unstable(id=\"{}\")", &job),
            format!("{} last builds with UNSTABLE", &job)
        )
        .unwrap();
        let grt = register_int_gauge!(
            format!("jenkins_job_reqtime_ms(id=\"{}\")", &job),
            format!("{} how long the last Jenkins API request took", &job)
        )
        .unwrap();
        gauges_total.insert(format!("{}", &job), gt);
        gauges_success.insert(format!("{}", &job), gs);
        gauges_failure.insert(format!("{}", &job), gf);
        gauges_unstable.insert(format!("{}", &job), gu);
        gauges_reqtime_ms.insert(format!("{}", &job), grt);
    }

    let mut wait_sec: u64 = 0;
    loop {
        let guard = exporter.wait_duration(std::time::Duration::from_secs(wait_sec));

        for job in &opts.jobs {
            let now = SystemTime::now();
            let response = get_job_builds(&opts, job);
            req_counter.inc();
            match now.elapsed() {
                Ok(elapsed) => {
                    let gkey = job.clone();
                    gauges_reqtime_ms[&gkey].set(elapsed.as_millis() as i64);
                }
                Err(e) => {
                    // an error occurred!
                    println!("Error: {:?}", e);
                }
            }
            match response {
                Err(e) => req_err_counter.inc(),
                Ok(r) => {
                    let (success_count, failure_count, unstable_count, total_count) =
                        calc_metrics(&r, opts.last_builds, opts.verbose);
                    let gkey = job.clone();
                    println!(
                        "{}: ok {}/ nok {}/ unstable {}/ total {}",
                        &job, success_count, failure_count, unstable_count, total_count
                    );
                    gauges_total[&gkey].set(total_count as i64);
                    gauges_success[&gkey].set(success_count as i64);
                    gauges_failure[&gkey].set(failure_count as i64);
                    gauges_unstable[&gkey].set(unstable_count as i64);
                }
            }
        }
        poll_counter.inc();
        drop(guard);
        wait_sec = opts.poll_interval_sec;
    }
}
