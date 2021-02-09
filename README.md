# jenkins-job-health-exporter
A Prometheus exporter that reports the relative health of selected Jenkins Jobs

# Installation

1. Install Rust as per https://www.rust-lang.org/tools/install

2. Clone this repository

3. Issue "cargo build" to compile it

4. Run the resulting executable:

```
$ ./target/debug/jenkins-job-health-exporter --help
jenkins-job-health-exporter 0.1
Andrew Yourtchenko <ayourtch@gmail.com>
This program periodically polls Jenkins jobs that are specified in the parameters, and exports it
for Prometheus

USAGE:
    jenkins-job-health-exporter [FLAGS] [OPTIONS] <jobs>... --jenkins-host <jenkins-host>

ARGS:
    <jobs>...    Jenkins jobs to monitor

FLAGS:
    -h, --help       Prints help information
    -v, --verbose    A level of verbosity, and can be used multiple times
    -V, --version    Prints version information

OPTIONS:
    -b, --bind-to <bind-to>
            Bind Prometheus exporter to this address [default: 127.0.0.1:9186]

    -j, --jenkins-host <jenkins-host>              Jenkins hostname to monitor the jobs on
    -l, --last-jobs <last-jobs>                    How many "last" jobs to look at [default: 10]
    -p, --poll-interval-sec <poll-interval-sec>
            Poll interval - how often to get the job builds status [default: 1800]

$ 


```


You can also output the configuration that the command line switches result in, as follows:


```
$ ./target/debug/jenkins-job-health-exporter -v -v -v -v -v -j example.com job1 job2
{
  "jenkins_host": "example.com",
  "req_timeout_sec": 30,
  "poll_interval_sec": 1800,
  "bind_to": "127.0.0.1:9186",
  "last_builds": 10,
  "jobs": [
    "job1",
    "job2"
  ],
  "verbose": 5
}
===========
---
jenkins_host: example.com
req_timeout_sec: 30
poll_interval_sec: 1800
bind_to: "127.0.0.1:9186"
last_builds: 10
jobs:
  - job1
  - job2
verbose: 5

Started Prometheus exporter on 127.0.0.1:9186, monitoring 2 jobs on example.com with 1800 seconds poll interval
```

You can then copy-paste either of the two outputs into a file, and if you give it as a sole parameter to the program, it will replace any
other options on the command line with the data from the config file:

```

$ ./target/debug/jenkins-job-health-exporter config.yaml
{
  "jenkins_host": "example.com",
  "req_timeout_sec": 30,
  "poll_interval_sec": 1800,
  "bind_to": "127.0.0.1:9186",
  "last_builds": 10,
  "jobs": [
    "job1",
    "job2"
  ],
  "verbose": 5
}
===========
---
jenkins_host: example.com
req_timeout_sec: 30
poll_interval_sec: 1800
bind_to: "127.0.0.1:9186"
last_builds: 10
jobs:
  - job1
  - job2
verbose: 5

Started Prometheus exporter on 127.0.0.1:9186, monitoring 2 jobs on example.com with 1800 seconds poll interval
```
