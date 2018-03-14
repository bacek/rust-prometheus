// Copyright 2014 The Prometheus Authors
// Copyright 2016 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

use atomic64::{Atomic, AtomicF64, AtomicI64, Number};
use desc::Desc;
use errors::Result;
use metrics::{Collector, Metric, Opts};
use proto;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use value::{Value, ValueType};
use vec::{MetricVec, MetricVecBuilder};

/// `Counter` is a Metric that represents a single numerical value that only ever
/// goes up.
pub struct GenericCounter<P: Atomic> {
    v: Arc<Value<P>>,
}

pub type Counter = GenericCounter<AtomicF64>;

pub type IntCounter = GenericCounter<AtomicI64>;

impl<P: Atomic> Clone for GenericCounter<P> {
    fn clone(&self) -> Self {
        Self {
            v: Arc::clone(&self.v),
        }
    }
}

impl<P: Atomic> GenericCounter<P> {
    /// `new` creates a `Counter` with the `name` and `help` arguments.
    pub fn new<S: Into<String>>(name: S, help: S) -> Result<Self> {
        let opts = Opts::new(name, help);
        Self::with_opts(opts)
    }

    /// `with_opts` creates a `Counter` with the `opts` options.
    pub fn with_opts(opts: Opts) -> Result<Self> {
        Self::with_opts_and_label_values(&opts, &[])
    }

    fn with_opts_and_label_values(opts: &Opts, label_values: &[&str]) -> Result<Self> {
        let v = Value::new(opts, ValueType::Counter, P::T::from_i64(0), label_values)?;
        Ok(Self { v: Arc::new(v) })
    }

    /// `inc_by` increments the given value to the counter.
    ///
    /// # Panics
    ///
    /// Panics if the value is < 0.
    #[inline]
    pub fn inc_by(&self, v: P::T) {
        if v < P::T::from_i64(0) {
            panic!("counter cannot inc negative values")
        }
        self.v.inc_by(v);
    }

    /// `inc` increments the counter by 1.
    #[inline]
    pub fn inc(&self) {
        self.v.inc();
    }

    /// `get` returns the counter value.
    #[inline]
    pub fn get(&self) -> P::T {
        self.v.get()
    }

    pub fn local(&self) -> GenericLocalCounter<P> {
        GenericLocalCounter::new(self.clone())
    }
}

impl<P: Atomic> Collector for GenericCounter<P> {
    fn desc(&self) -> Vec<&Desc> {
        vec![&self.v.desc]
    }

    fn collect(&self) -> Vec<proto::MetricFamily> {
        vec![self.v.collect()]
    }
}

impl<P: Atomic> Metric for GenericCounter<P> {
    fn metric(&self) -> proto::Metric {
        self.v.metric()
    }
}

pub struct CounterVecBuilder<P: Atomic> {
    _phantom: PhantomData<P>,
}

impl<P: Atomic> CounterVecBuilder<P> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<P: Atomic> Clone for CounterVecBuilder<P> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<P: Atomic> MetricVecBuilder for CounterVecBuilder<P> {
    type M = GenericCounter<P>;
    type P = Opts;

    fn build(&self, opts: &Opts, vals: &[&str]) -> Result<Self::M> {
        Self::M::with_opts_and_label_values(opts, vals)
    }
}

/// `CounterVec` is a Collector that bundles a set of Counters that all share the
/// same Desc, but have different values for their variable labels. This is used
/// if you want to count the same thing partitioned by various dimensions
/// (e.g. number of HTTP requests, partitioned by response code and method).
pub type GenericCounterVec<P> = MetricVec<CounterVecBuilder<P>>;

pub type CounterVec = GenericCounterVec<AtomicF64>;

pub type IntCounterVec = GenericCounterVec<AtomicI64>;

impl<P: Atomic> GenericCounterVec<P> {
    /// `new` creates a new `CounterVec` based on the provided `Opts` and
    /// partitioned by the given label names. At least one label name must be
    /// provided.
    pub fn new(opts: Opts, label_names: &[&str]) -> Result<Self> {
        let variable_names = label_names.iter().map(|s| (*s).to_owned()).collect();
        let opts = opts.variable_labels(variable_names);
        let metric_vec =
            MetricVec::create(proto::MetricType::COUNTER, CounterVecBuilder::new(), opts)?;

        Ok(metric_vec as Self)
    }

    pub fn local(&self) -> GenericLocalCounterVec<P> {
        GenericLocalCounterVec::new(self.clone())
    }
}

pub struct GenericLocalCounter<P: Atomic> {
    counter: GenericCounter<P>,
    val: P::T,
}

pub type LocalCounter = GenericLocalCounter<AtomicF64>;

pub type LocalIntCounter = GenericLocalCounter<AtomicI64>;

// LocalCounter is a thread local copy of Counter
impl<P: Atomic> GenericLocalCounter<P> {
    fn new(counter: GenericCounter<P>) -> Self {
        Self {
            counter,
            val: P::T::from_i64(0),
        }
    }

    /// `inc_by` increments the given value to the local counter.
    ///
    /// # Panics
    ///
    /// Panics if the value is < 0.
    #[inline]
    pub fn inc_by(&mut self, v: P::T) {
        if v < P::T::from_i64(0) {
            panic!("counter cannot inc negative values")
        }
        self.val += v;
    }

    /// `inc` increments the local counter by 1.
    #[inline]
    pub fn inc(&mut self) {
        self.val += P::T::from_i64(1);
    }

    /// `get` returns the local counter value.
    #[inline]
    pub fn get(&self) -> P::T {
        self.val
    }

    /// `flush` the local counter value to the counter
    #[inline]
    pub fn flush(&mut self) {
        if self.val == P::T::from_i64(0) {
            return;
        }
        self.counter.inc_by(self.val);
        self.val = P::T::from_i64(0);
    }
}

impl<P: Atomic> Clone for GenericLocalCounter<P> {
    fn clone(&self) -> Self {
        Self::new(self.counter.clone())
    }
}

pub struct GenericLocalCounterVec<P: Atomic> {
    vec: GenericCounterVec<P>,
    local: HashMap<u64, GenericLocalCounter<P>>,
}

pub type LocalCounterVec = GenericLocalCounterVec<AtomicF64>;

pub type LocalIntCounterVec = GenericLocalCounterVec<AtomicI64>;

impl<P: Atomic> GenericLocalCounterVec<P> {
    fn new(vec: GenericCounterVec<P>) -> Self {
        let local = HashMap::with_capacity(vec.v.children.read().len());
        Self { vec, local }
    }

    /// Get a `LocalCounter` by label values.
    /// See more [MetricVec::with_label_values]
    /// (/prometheus/struct.MetricVec.html#method.with_label_values)
    pub fn with_label_values<'a>(&'a mut self, vals: &[&str]) -> &'a mut GenericLocalCounter<P> {
        let hash = self.vec.v.hash_label_values(vals).unwrap();
        let vec = &self.vec;
        self.local
            .entry(hash)
            .or_insert_with(|| vec.with_label_values(vals).local())
    }

    /// Remove a `LocalCounter` by label values.
    /// See more [MetricVec::remove_label_values]
    /// (/prometheus/struct.MetricVec.html#method.remove_label_values)
    pub fn remove_label_values(&mut self, vals: &[&str]) -> Result<()> {
        let hash = self.vec.v.hash_label_values(vals)?;
        self.local.remove(&hash);
        self.vec.v.delete_label_values(vals)
    }

    /// `flush` flushes the local metrics to the CounterVec metric.
    pub fn flush(&mut self) {
        for h in self.local.values_mut() {
            h.flush();
        }
    }
}

impl<P: Atomic> Clone for GenericLocalCounterVec<P> {
    fn clone(&self) -> Self {
        Self::new(self.vec.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::{Collector, Opts};
    use std::collections::HashMap;
    use std::f64::EPSILON;

    #[test]
    fn test_counter() {
        let opts = Opts::new("test", "test help")
            .const_label("a", "1")
            .const_label("b", "2");
        let counter = Counter::with_opts(opts).unwrap();
        counter.inc();
        assert_eq!(counter.get() as u64, 1);
        counter.inc_by(42.0);
        assert_eq!(counter.get() as u64, 43);

        let mut mfs = counter.collect();
        assert_eq!(mfs.len(), 1);

        let mf = mfs.pop().unwrap();
        let m = mf.get_metric().get(0).unwrap();
        assert_eq!(m.get_label().len(), 2);
        assert_eq!(m.get_counter().get_value() as u64, 43);
    }

    #[test]
    fn test_int_counter() {
        let counter = IntCounter::new("foo", "bar").unwrap();
        counter.inc();
        assert_eq!(counter.get(), 1);
        counter.inc_by(11);
        assert_eq!(counter.get(), 12);

        let mut mfs = counter.collect();
        assert_eq!(mfs.len(), 1);

        let mf = mfs.pop().unwrap();
        let m = mf.get_metric().get(0).unwrap();
        assert_eq!(m.get_label().len(), 0);
        assert_eq!(m.get_counter().get_value() as u64, 12);
    }

    #[test]
    fn test_local_counter() {
        let counter = Counter::new("counter", "counter helper").unwrap();
        let mut local_counter1 = counter.local();
        let mut local_counter2 = counter.local();

        local_counter1.inc();
        local_counter2.inc();
        assert_eq!(local_counter1.get() as u64, 1);
        assert_eq!(local_counter2.get() as u64, 1);
        assert_eq!(counter.get() as u64, 0);
        local_counter1.flush();
        assert_eq!(local_counter1.get() as u64, 0);
        assert_eq!(counter.get() as u64, 1);
        local_counter2.flush();
        assert_eq!(counter.get() as u64, 2);
    }

    #[test]
    fn test_int_local_counter() {
        let counter = IntCounter::new("foo", "bar").unwrap();
        let mut local_counter = counter.local();

        local_counter.inc();
        assert_eq!(local_counter.get(), 1);
        assert_eq!(counter.get(), 0);

        local_counter.inc_by(5);
        local_counter.flush();
        assert_eq!(local_counter.get(), 0);
        assert_eq!(counter.get(), 6);
    }

    #[test]
    fn test_counter_vec_with_labels() {
        let vec = CounterVec::new(
            Opts::new("test_couter_vec", "test counter vec help"),
            &["l1", "l2"],
        ).unwrap();

        let mut labels = HashMap::new();
        labels.insert("l1", "v1");
        labels.insert("l2", "v2");
        assert!(vec.remove(&labels).is_err());

        vec.with(&labels).inc();
        assert!(vec.remove(&labels).is_ok());
        assert!(vec.remove(&labels).is_err());

        let mut labels2 = HashMap::new();
        labels2.insert("l1", "v2");
        labels2.insert("l2", "v1");

        vec.with(&labels).inc();
        assert!(vec.remove(&labels2).is_err());

        vec.with(&labels).inc();

        let mut labels3 = HashMap::new();
        labels3.insert("l1", "v1");
        assert!(vec.remove(&labels3).is_err());
    }

    #[test]
    fn test_int_counter_vec() {
        let vec = IntCounterVec::new(Opts::new("foo", "bar"), &["l1", "l2"]).unwrap();

        vec.with_label_values(&["v1", "v3"]).inc();
        assert_eq!(vec.with_label_values(&["v1", "v3"]).get(), 1);

        vec.with_label_values(&["v1", "v2"]).inc_by(12);
        assert_eq!(vec.with_label_values(&["v1", "v3"]).get(), 1);
        assert_eq!(vec.with_label_values(&["v1", "v2"]).get(), 12);

        vec.with_label_values(&["v4", "v2"]).inc_by(2);
        assert_eq!(vec.with_label_values(&["v1", "v3"]).get(), 1);
        assert_eq!(vec.with_label_values(&["v1", "v2"]).get(), 12);
        assert_eq!(vec.with_label_values(&["v4", "v2"]).get(), 2);

        vec.with_label_values(&["v1", "v3"]).inc_by(5);
        assert_eq!(vec.with_label_values(&["v1", "v3"]).get(), 6);
        assert_eq!(vec.with_label_values(&["v1", "v2"]).get(), 12);
        assert_eq!(vec.with_label_values(&["v4", "v2"]).get(), 2);
    }

    #[test]
    fn test_counter_vec_with_label_values() {
        let vec = CounterVec::new(
            Opts::new("test_vec", "test counter vec help"),
            &["l1", "l2"],
        ).unwrap();

        assert!(vec.remove_label_values(&["v1", "v2"]).is_err());
        vec.with_label_values(&["v1", "v2"]).inc();
        assert!(vec.remove_label_values(&["v1", "v2"]).is_ok());

        vec.with_label_values(&["v1", "v2"]).inc();
        assert!(vec.remove_label_values(&["v1"]).is_err());
        assert!(vec.remove_label_values(&["v1", "v3"]).is_err());
    }

    #[test]
    fn test_counter_vec_local() {
        let vec = CounterVec::new(
            Opts::new("test_vec_local", "test counter vec help"),
            &["l1", "l2"],
        ).unwrap();
        let mut local_vec_1 = vec.local();
        let mut local_vec_2 = local_vec_1.clone();

        assert!(local_vec_1.remove_label_values(&["v1", "v2"]).is_err());

        local_vec_1.with_label_values(&["v1", "v2"]).inc_by(23.0);
        assert!((local_vec_1.with_label_values(&["v1", "v2"]).get() - 23.0) <= EPSILON);
        assert!((vec.with_label_values(&["v1", "v2"]).get() - 0.0) <= EPSILON);

        local_vec_1.flush();
        assert!((local_vec_1.with_label_values(&["v1", "v2"]).get() - 0.0) <= EPSILON);
        assert!((vec.with_label_values(&["v1", "v2"]).get() - 23.0) <= EPSILON);

        local_vec_1.flush();
        assert!((local_vec_1.with_label_values(&["v1", "v2"]).get() - 0.0) <= EPSILON);
        assert!((vec.with_label_values(&["v1", "v2"]).get() - 23.0) <= EPSILON);

        local_vec_1.with_label_values(&["v1", "v2"]).inc_by(11.0);
        assert!((local_vec_1.with_label_values(&["v1", "v2"]).get() - 11.0) <= EPSILON);
        assert!((vec.with_label_values(&["v1", "v2"]).get() - 23.0) <= EPSILON);

        local_vec_1.flush();
        assert!((local_vec_1.with_label_values(&["v1", "v2"]).get() - 0.0) <= EPSILON);
        assert!((vec.with_label_values(&["v1", "v2"]).get() - 34.0) <= EPSILON);

        // When calling `remove_label_values`, it is "flushed" immediately.
        assert!(local_vec_1.remove_label_values(&["v1", "v2"]).is_ok());
        assert!((local_vec_1.with_label_values(&["v1", "v2"]).get() - 0.0) <= EPSILON);
        assert!((vec.with_label_values(&["v1", "v2"]).get() - 0.0) <= EPSILON);

        local_vec_1.with_label_values(&["v1", "v2"]).inc();
        assert!(local_vec_1.remove_label_values(&["v1"]).is_err());
        assert!(local_vec_1.remove_label_values(&["v1", "v3"]).is_err());

        local_vec_1.with_label_values(&["v1", "v2"]).inc_by(13.0);
        assert!((local_vec_1.with_label_values(&["v1", "v2"]).get() - 14.0) <= EPSILON);
        assert!((vec.with_label_values(&["v1", "v2"]).get() - 0.0) <= EPSILON);

        local_vec_2.with_label_values(&["v1", "v2"]).inc_by(7.0);
        assert!((local_vec_2.with_label_values(&["v1", "v2"]).get() - 7.0) <= EPSILON);

        local_vec_1.flush();
        local_vec_2.flush();
        assert!((vec.with_label_values(&["v1", "v2"]).get() - 21.0) <= EPSILON);

        local_vec_1.flush();
        local_vec_2.flush();
        assert!((vec.with_label_values(&["v1", "v2"]).get() - 21.0) <= EPSILON);
    }

    #[test]
    fn test_int_counter_vec_local() {
        let vec = IntCounterVec::new(Opts::new("foo", "bar"), &["l1", "l2"]).unwrap();
        let mut local_vec_1 = vec.local();
        assert!(local_vec_1.remove_label_values(&["v1", "v2"]).is_err());

        local_vec_1.with_label_values(&["v1", "v2"]).inc_by(23);
        assert_eq!(local_vec_1.with_label_values(&["v1", "v2"]).get(), 23);
        assert_eq!(vec.with_label_values(&["v1", "v2"]).get(), 0);

        local_vec_1.flush();
        assert_eq!(local_vec_1.with_label_values(&["v1", "v2"]).get(), 0);
        assert_eq!(vec.with_label_values(&["v1", "v2"]).get(), 23);

        local_vec_1.flush();
        assert_eq!(local_vec_1.with_label_values(&["v1", "v2"]).get(), 0);
        assert_eq!(vec.with_label_values(&["v1", "v2"]).get(), 23);

        local_vec_1.with_label_values(&["v1", "v2"]).inc_by(11);
        assert_eq!(local_vec_1.with_label_values(&["v1", "v2"]).get(), 11);
        assert_eq!(vec.with_label_values(&["v1", "v2"]).get(), 23);

        local_vec_1.flush();
        assert_eq!(local_vec_1.with_label_values(&["v1", "v2"]).get(), 0);
        assert_eq!(vec.with_label_values(&["v1", "v2"]).get(), 34);
    }

    #[test]
    #[should_panic(expected = "counter cannot inc negative values")]
    fn test_counter_negative_inc() {
        let counter = Counter::new("foo", "bar").unwrap();
        counter.inc_by(-42.0);
    }

    #[test]
    #[should_panic(expected = "counter cannot inc negative values")]
    fn test_local_counter_negative_inc() {
        let counter = Counter::new("foo", "bar").unwrap();
        let mut local = counter.local();
        local.inc_by(-42.0);
    }

    #[test]
    #[should_panic(expected = "counter cannot inc negative values")]
    fn test_int_counter_negative_inc() {
        let counter = IntCounter::new("foo", "bar").unwrap();
        counter.inc_by(-42);
    }

    #[test]
    #[should_panic(expected = "counter cannot inc negative values")]
    fn test_int_local_counter_negative_inc() {
        let counter = IntCounter::new("foo", "bar").unwrap();
        let mut local = counter.local();
        local.inc_by(-42);
    }
}
