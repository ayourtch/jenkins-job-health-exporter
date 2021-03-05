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
#[derive(Clone, Clap, Serialize, Deserialize)]
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

fn calc_metrics(
    data: &Result<AllBuilds, MyError>,
    try_total: usize,
    verbose: i32,
) -> HashMap<String, i64> {
    let statuses = vec!["success", "failure", "unstable"];

    let mut out = HashMap::new();
    out.insert("total".into(), 0);
    for s in &statuses {
        out.insert(s.to_string(), 0);
    }

    if data.is_err() {
        return out;
    }

    let data = data.as_ref().unwrap();
    let last_n = data.builds.windows(try_total).nth(0);
    if last_n.is_none() {
        return out;
    }

    let last_n = last_n.unwrap();
    if verbose > 4 {
        println!("all data: {:#?}", data);
        println!("last data: {:#?}", &last_n);
    }

    out.insert("total".into(), last_n.len() as i64);
    for st in &statuses {
        let val = last_n
            .iter()
            .filter(|x| {
                if let Some(res) = &x.result {
                    res == &st.to_uppercase()
                } else {
                    false
                }
            })
            .count();
        out.insert(st.to_string(), val as i64);
    }
    return out;
}

#[derive(Clone, Debug, Default)]
struct AllGaugeData {
    gauges: HashMap<String, HashMap<String, i64>>,
    req_counter: i64,
    req_err_counter: i64,
}

fn get_all_gauge_data(opts: &Opts) -> AllGaugeData {
    let mut out = AllGaugeData {
        ..Default::default()
    };
    for job in &opts.jobs {
        let now = SystemTime::now();
        out.req_counter = out.req_counter + 1;
        let response = get_job_builds(&opts, job);
        let elapsed = match now.elapsed() {
            Ok(elapsed) => elapsed.as_millis() as i64,
            Err(e) => {
                // an error occurred!
                println!("Error: {:?}", e);
                -1
            }
        };
        if response.is_err() {
            out.req_err_counter = out.req_err_counter + 1;
        }
        let metrics = calc_metrics(&response, opts.last_builds, opts.verbose);
        println!(
            "{}: ok {}/ nok {}/ unstable {}/ total {}",
            &job, &metrics["success"], &metrics["failure"], &metrics["unstable"], &metrics["total"]
        );
        out.gauges.insert(job.to_string(), metrics);
        out.gauges
            .get_mut(job)
            .unwrap()
            .insert("job_reqtime_ms".to_string(), elapsed);
    }
    out
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

    let mut gauges: HashMap<String, HashMap<String, GenericGauge<AtomicI64>>> = HashMap::new();
    let gauge_info = vec![
        ("total", "last builds total"),
        ("success", "last builds with SUCCESS"),
        ("failure", "last builds with FAILURE"),
        ("unstable", "last builds with UNSTABLE"),
        (
            "job_reqtime_ms",
            "how long the last Jenkins API request took",
        ),
    ];

    for job in &opts.jobs {
        for (gauge_name, gauge_help) in &gauge_info {
            let metric_name = job.clone().replace("-", "_");
            let new_gauge = register_int_gauge!(
                format!("{}_{}", &metric_name, gauge_name),
                format!("{} {}", &job, &gauge_help)
            )
            .unwrap();
            gauges
                .entry(job.to_string())
                .or_insert(HashMap::new())
                .insert(gauge_name.to_string(), new_gauge);
        }
    }

    let mut wait_sec: u64 = 0;
    loop {
        let opts_clone = opts.clone();
        let handle = std::thread::spawn(move || {
            let opts = opts_clone;
            let new_data = get_all_gauge_data(&opts);
            if opts.verbose > 3 {
                eprintln!("d: {:#?}", &new_data);
            }
            new_data
        });

        let guard = exporter.wait_duration(std::time::Duration::from_secs(wait_sec));
        let new_data = handle.join().unwrap();

        for job in &opts.jobs {
            for (gauge_name, _) in &gauge_info {
                /*(
                # we pre-created the hashmaps on the left, and we expect
                # the same data from hashmaps on the right,
                # if the data is not there this is a terminal event
                */
                if opts.verbose > 4 {
                    eprintln!("fill job: {} gauge: {}", &job, &gauge_name);
                }
                let d = new_data.gauges[&job.to_string()][&gauge_name.to_string()];

                gauges[&job.to_string()][&gauge_name.to_string()].set(d);
            }
        }
        req_err_counter.inc_by(new_data.req_err_counter as f64);
        req_counter.inc_by(new_data.req_counter as f64);

        poll_counter.inc();
        drop(guard);
        wait_sec = opts.poll_interval_sec;
    }
}
