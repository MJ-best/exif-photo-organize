//! Asynchronous texture cache.
//!
//! The UI asks for images every frame (`get`). Missing ones are queued in visible order; worker
//! threads decode them and the UI uploads at most a few textures per frame to keep scrolling
//! smooth. The queue is *replaced* every frame, so images that scrolled out of view before they
//! were decoded are never decoded at all.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

pub enum Loaded {
    Image(ColorImage),
    /// Nothing to show (yet) — e.g. thumbnail not generated.
    Missing,
}

pub type Loader<K> = Box<dyn FnMut(K) -> Loaded + Send>;
pub type LoaderFactory<K> = Arc<dyn Fn() -> Loader<K> + Send + Sync>;

struct Shared<K> {
    queue: VecDeque<K>,
    inflight: HashSet<K>,
    done: Vec<(K, Loaded)>,
}

struct Inner<K> {
    shared: Mutex<Shared<K>>,
    cv: Condvar,
    stop: AtomicBool,
}

struct Entry {
    tex: TextureHandle,
    last_used: u64,
}

pub struct ImageCache<K: Copy + Eq + Hash + Send + 'static> {
    name: &'static str,
    inner: Arc<Inner<K>>,
    tex: HashMap<K, Entry>,
    missing: HashMap<K, f64>,
    wanted: Vec<K>,
    wanted_set: HashSet<K>,
    frame: u64,
    capacity: usize,
    uploads_per_frame: usize,
    options: TextureOptions,
}

impl<K: Copy + Eq + Hash + Send + std::fmt::Debug + 'static> ImageCache<K> {
    pub fn new(
        name: &'static str,
        ctx: &egui::Context,
        threads: usize,
        capacity: usize,
        factory: LoaderFactory<K>,
    ) -> Self {
        let inner = Arc::new(Inner {
            shared: Mutex::new(Shared { queue: VecDeque::new(), inflight: HashSet::new(), done: Vec::new() }),
            cv: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        for i in 0..threads.max(1) {
            let inner = inner.clone();
            let ctx = ctx.clone();
            let factory = factory.clone();
            std::thread::Builder::new()
                .name(format!("{name}-{i}"))
                .spawn(move || {
                    let mut load = factory();
                    loop {
                        let key = {
                            let mut s = inner.shared.lock().unwrap();
                            loop {
                                if inner.stop.load(Ordering::Relaxed) {
                                    return;
                                }
                                if let Some(k) = s.queue.pop_front() {
                                    s.inflight.insert(k);
                                    break k;
                                }
                                s = inner.cv.wait(s).unwrap();
                            }
                        };
                        let result = load(key);
                        {
                            let mut s = inner.shared.lock().unwrap();
                            s.inflight.remove(&key);
                            s.done.push((key, result));
                        }
                        ctx.request_repaint();
                    }
                })
                .expect("spawn image loader");
        }
        ImageCache {
            name,
            inner,
            tex: HashMap::new(),
            missing: HashMap::new(),
            wanted: Vec::new(),
            wanted_set: HashSet::new(),
            frame: 0,
            capacity,
            uploads_per_frame: 24,
            options: TextureOptions::LINEAR,
        }
    }

    pub fn with_uploads_per_frame(mut self, n: usize) -> Self {
        self.uploads_per_frame = n;
        self
    }

    /// Call once per frame before any `get`.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        let now = ctx.input(|i| i.time);
        let done: Vec<(K, Loaded)> = {
            let mut s = self.inner.shared.lock().unwrap();
            let n = s.done.len().min(self.uploads_per_frame);
            s.done.drain(..n).collect()
        };
        let more = !self.inner.shared.lock().unwrap().done.is_empty();
        for (k, r) in done {
            match r {
                Loaded::Image(img) => {
                    let tex = ctx.load_texture(format!("{}-{k:?}", self.name), img, self.options);
                    self.tex.insert(k, Entry { tex, last_used: self.frame });
                    self.missing.remove(&k);
                }
                Loaded::Missing => {
                    self.missing.insert(k, now);
                }
            }
        }
        if more {
            ctx.request_repaint();
        }
        // Retry missing entries after a while (thumbnails may be generated meanwhile).
        self.missing.retain(|_, t| now - *t < 5.0);
    }

    /// Texture for `key` if loaded; otherwise schedules loading.
    pub fn get(&mut self, key: K) -> Option<TextureHandle> {
        if let Some(e) = self.tex.get_mut(&key) {
            e.last_used = self.frame;
            return Some(e.tex.clone());
        }
        if !self.missing.contains_key(&key) && self.wanted_set.insert(key) {
            self.wanted.push(key);
        }
        None
    }

    pub fn is_missing(&self, key: K) -> bool {
        self.missing.contains_key(&key)
    }

    /// Schedule a load without needing the texture now (prefetch). Lower priority than `get`.
    /// A prefetched texture that is already loaded counts as used, so it is not evicted first.
    pub fn prefetch(&mut self, key: K) {
        if let Some(e) = self.tex.get_mut(&key) {
            e.last_used = self.frame;
        } else if !self.missing.contains_key(&key) && self.wanted_set.insert(key) {
            self.wanted.push(key);
        }
    }

    /// Call once per frame after all `get`s: publishes the wanted list and evicts old textures.
    pub fn end_frame(&mut self) {
        {
            let mut s = self.inner.shared.lock().unwrap();
            let inflight = s.inflight.clone();
            s.queue.clear();
            for k in self.wanted.drain(..) {
                if !inflight.contains(&k) {
                    s.queue.push_back(k);
                }
            }
            if !s.queue.is_empty() {
                self.inner.cv.notify_all();
            }
        }
        self.wanted_set.clear();
        if self.tex.len() > self.capacity {
            let mut v: Vec<(K, u64)> = self.tex.iter().map(|(k, e)| (*k, e.last_used)).collect();
            v.sort_by_key(|x| x.1);
            let excess = self.tex.len() - self.capacity;
            for (k, used) in v.into_iter().take(excess) {
                if used + 2 < self.frame {
                    self.tex.remove(&k);
                }
            }
        }
    }

    pub fn invalidate(&mut self, key: K) {
        self.tex.remove(&key);
        self.missing.remove(&key);
    }

    /// Forget "missing" markers so they are retried immediately.
    pub fn retry_missing(&mut self, key: K) {
        self.missing.remove(&key);
    }

    pub fn clear(&mut self) {
        self.tex.clear();
        self.missing.clear();
    }
}

impl<K: Copy + Eq + Hash + Send + 'static> Drop for ImageCache<K> {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        self.inner.cv.notify_all();
    }
}

pub fn color_image(img: &photoorg_core::image::RgbImage) -> ColorImage {
    let (w, h) = img.dimensions();
    ColorImage::from_rgb([w as usize, h as usize], img.as_raw())
}
