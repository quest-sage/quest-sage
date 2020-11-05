//! The `profile` module provides utilities for profiling often-called functions.

use std::collections::HashMap;
use std::time::{Duration, Instant};

pub type TaskName = &'static str;

pub fn display_time(seconds: f64) -> String {
    let (time, time_unit) = if seconds >= 1.0 {
        (seconds, " ")
    } else if seconds >= 0.001 {
        (seconds * 1000.0, "m")
    } else if seconds >= 0.000_001 {
        (seconds * 1000_000.0, "\u{03BC}") // micro SI prefix (Greek lowercase mu)
    } else {
        (seconds * 1000_000_000.0, "n")
    };
    format!("{:3}{}s", time as u32, time_unit)
}

/// Allows profiling of events that happen repeatedly in a roughly predictable manner.
/// Profiling using this object allows you to see which parts of a function normally take more
/// time than others over the course of many invocations of the function.
pub struct CycleProfiler {
    /// Represents the entire body of the computation.
    pub main_segment: ProfileSegment,

    /// Times between iterations of the main segment to
    /// deduce how much time was spent not actually doing profiled stuff.
    pub stopwatch: InterpolatedStopwatch,
}

impl CycleProfiler {
    pub fn new(interpolation_amount: usize) -> Self {
        Self {
            main_segment: ProfileSegment::new(interpolation_amount),
            stopwatch: InterpolatedStopwatch::new(interpolation_amount),
        }
    }
}

impl std::fmt::Display for CycleProfiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.main_segment.ticks < self.main_segment.interpolation_amount as u64 {
            return writeln!(f, "Insufficient data");
        }
        let total_time = self.stopwatch.average_time().as_secs_f64();
        let calculation_time = self.main_segment.average_time();
        writeln!(
            f,
            "Total time elapsed: {} / {}, {:5.2}% of total CPU time",
            display_time(calculation_time),
            display_time(total_time),
            100.0 * calculation_time / total_time
        )?;
        self.main_segment.display(f, 0)
    }
}

/// Represents the time it took to complete a particular block of code.
/// This counts the durations of intervals of time, and calculates the average
/// duration, by storing the durations of the last `n` intervals, where `n` is some arbitrary
/// constant specified in the stopwatch constructor.
pub struct ProfileSegment {
    interpolation_amount: usize,
    sub_tasks: HashMap<TaskName, ProfileSegment>,
    durations_seconds: Vec<f64>,
    offset: usize,
    pub ticks: u64,
}

impl ProfileSegment {
    fn new(interpolation_amount: usize) -> Self {
        Self {
            interpolation_amount,
            sub_tasks: HashMap::new(),
            durations_seconds: vec![1.0; interpolation_amount],
            offset: 0,
            ticks: 0,
        }
    }

    fn display(&self, f: &mut std::fmt::Formatter<'_>, indent: usize) -> std::fmt::Result {
        let total_duration = self.average_time();
        // E.g. [indent] 5.32% 132ms: some_task
        for (task_name, task) in &self.sub_tasks {
            let time_seconds = task.average_time();
            let percentage = 100.0 * time_seconds / total_duration;
            writeln!(
                f,
                "{:indent$}{:5.2}% {}: {}",
                "",
                percentage,
                display_time(time_seconds),
                task_name,
                indent = indent
            )?;
            task.display(f, indent + 4)?;
        }

        Ok(())
    }

    /// Call this function every time the given event happens, supplying the duration of the interval.
    fn tick(&mut self, duration: f64) {
        self.durations_seconds[self.offset] = duration;
        self.offset = (self.offset + 1) % self.durations_seconds.len();
        self.ticks += 1;
    }

    pub fn time<'a>(&'a mut self) -> ProfileSegmentGuard<'a> {
        ProfileSegmentGuard {
            start_instant: Instant::now(),
            segment: self,
        }
    }

    /// Returns an amount of seconds.
    pub fn average_time(&self) -> f64 {
        self.durations_seconds.iter().copied().sum::<f64>() / self.durations_seconds.len() as f64
    }
}

impl std::fmt::Display for ProfileSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.display(f, 0)
    }
}

/// Times the duration of an event. When dropped, the duration of this struct's life
/// will be sent to the segment.
pub struct ProfileSegmentGuard<'a> {
    start_instant: Instant,
    segment: &'a mut ProfileSegment,
}

impl Drop for ProfileSegmentGuard<'_> {
    fn drop(&mut self) {
        self.segment.tick(
            Instant::now()
                .duration_since(self.start_instant)
                .as_secs_f64(),
        );
    }
}

impl<'a> ProfileSegmentGuard<'a> {
    pub fn task(&mut self, name: TaskName) -> &mut ProfileSegment {
        let interpolation_amount = self.segment.interpolation_amount;
        self.segment
            .sub_tasks
            .entry(name)
            .or_insert_with(|| ProfileSegment::new(interpolation_amount))
    }
}

/// An interpolated stopwatch counts the time between successive events, and calculates the average
/// time between those events, by storing the times of the last `n` events, where `n` is some arbitrary
/// constant specified in the stopwatch constructor.
pub struct InterpolatedStopwatch {
    times: Vec<Instant>,
    offset: usize,
    pub ticks: u64,
}

impl InterpolatedStopwatch {
    pub fn new(interpolation_amount: usize) -> InterpolatedStopwatch {
        InterpolatedStopwatch {
            times: vec![Instant::now(); interpolation_amount],
            offset: 0,
            ticks: 0,
        }
    }

    /// Call this function every time the given event happens.
    /// You will be able to retrieve the average time between calls to `tick`
    /// using the `average_time` function.
    ///
    /// Returns the time between the previous tick and this tick.
    pub fn tick(&mut self) -> Duration {
        let prev_offset = match self.offset {
            0 => self.times.len() - 1,
            _ => self.offset - 1,
        };

        self.times[self.offset] = Instant::now();
        let old_time = self.times[prev_offset];
        let time = self.times[self.offset].duration_since(old_time);
        self.offset = (self.offset + 1) % self.times.len();
        self.ticks += 1;
        time
    }

    pub fn average_time(&self) -> Duration {
        let prev_offset = match self.offset {
            0 => self.times.len() - 1,
            _ => self.offset - 1,
        };
        self.times[prev_offset]
            .duration_since(self.times[self.offset])
            .div_f64(self.times.len() as f64)
    }
}
