//! Captura del audio que está sonando (monitor de la salida por defecto) y
//! cálculo del espectro para las barras.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rustfft::{num_complex::Complex, Fft, FftPlanner};

pub const RATE: u32 = 48_000;
pub const FFT_SIZE: usize = 2048;

const F_LO: f32 = 40.0;
const F_HI: f32 = 14_000.0;

/// Últimas `FFT_SIZE` muestras recibidas (búfer circular).
pub struct Ring {
    buf: Vec<f32>,
    pos: usize,
}

impl Ring {
    fn new(n: usize) -> Self {
        Ring { buf: vec![0.0; n], pos: 0 }
    }

    fn push(&mut self, s: f32) {
        self.buf[self.pos] = s;
        self.pos = (self.pos + 1) % self.buf.len();
    }

    fn copy_ordered(&self, out: &mut [f32]) {
        let (old, new) = self.buf.split_at(self.pos);
        out[..new.len()].copy_from_slice(new);
        out[new.len()..].copy_from_slice(old);
    }
}

fn default_sink() -> Option<String> {
    let out = Command::new("pactl").arg("get-default-sink").output().ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Lanza `parec` sobre el monitor de la salida por defecto y lo relanza si
/// cambia la salida (p. ej. al conectar los cascos bluetooth).
pub fn spawn_capture() -> Arc<Mutex<Ring>> {
    let ring = Arc::new(Mutex::new(Ring::new(FFT_SIZE)));
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::default();

    {
        let child_slot = child_slot.clone();
        thread::spawn(move || {
            let mut last = default_sink();
            loop {
                thread::sleep(Duration::from_secs(2));
                let now = default_sink();
                if now != last {
                    last = now;
                    if let Some(child) = child_slot.lock().unwrap().as_mut() {
                        let _ = child.kill();
                    }
                }
            }
        });
    }

    {
        let ring = ring.clone();
        thread::spawn(move || loop {
            let device = default_sink()
                .map(|s| format!("{s}.monitor"))
                .unwrap_or_else(|| "@DEFAULT_MONITOR@".into());
            let spawned = Command::new("parec")
                .args([
                    "-d",
                    &device,
                    "--raw",
                    "--format=float32le",
                    &format!("--rate={RATE}"),
                    "--channels=1",
                    "--latency-msec=20",
                    "--client-name=ambient-visualizer",
                    "--stream-name=Visualizador",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();

            if let Ok(mut child) = spawned {
                let mut stdout = child.stdout.take().unwrap();
                *child_slot.lock().unwrap() = Some(child);

                let mut bytes = [0u8; 1024];
                while stdout.read_exact(&mut bytes).is_ok() {
                    let mut ring = ring.lock().unwrap();
                    for chunk in bytes.chunks_exact(4) {
                        ring.push(f32::from_le_bytes(chunk.try_into().unwrap()));
                    }
                }

                if let Some(mut child) = child_slot.lock().unwrap().take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            thread::sleep(Duration::from_millis(500));
        });
    }

    ring
}

/// Convierte las muestras en alturas de barra (0..1) suavizadas.
pub struct Spectrum {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    samples: Vec<f32>,
    buf: Vec<Complex<f32>>,
    /// Rango de bins (fraccionario) de cada barra.
    bands: Vec<(f32, f32)>,
    weights: Vec<f32>,
    raw: Vec<f32>,
    velocity: Vec<f32>,
    pub bars: Vec<f32>,
    pub peaks: Vec<f32>,
    sens: f32,
}

impl Spectrum {
    pub fn new(n: usize) -> Self {
        let bin = |f: f32| f * FFT_SIZE as f32 / RATE as f32;
        let edge = |i: usize| F_LO * (F_HI / F_LO).powf(i as f32 / n as f32);
        let bands = (0..n).map(|i| (bin(edge(i)), bin(edge(i + 1)))).collect();
        // La música tiene mucha más energía en graves: se compensa un poco para
        // que los agudos también se muevan.
        let weights = (0..n)
            .map(|i| {
                let center = (edge(i) * edge(i + 1)).sqrt();
                (center / F_LO).powf(0.42)
            })
            .collect();
        let window = (0..FFT_SIZE)
            .map(|i| {
                let x = i as f32 / (FFT_SIZE - 1) as f32;
                0.5 - 0.5 * (std::f32::consts::TAU * x).cos()
            })
            .collect();

        Spectrum {
            fft: FftPlanner::new().plan_fft_forward(FFT_SIZE),
            window,
            samples: vec![0.0; FFT_SIZE],
            buf: vec![Complex::default(); FFT_SIZE],
            bands,
            weights,
            raw: vec![0.0; n],
            velocity: vec![0.0; n],
            bars: vec![0.0; n],
            peaks: vec![0.0; n],
            sens: 1.0,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.bars.iter().chain(&self.peaks).all(|v| *v < 0.002)
    }

    pub fn update(&mut self, ring: &Mutex<Ring>, dt: f32) {
        ring.lock().unwrap().copy_ordered(&mut self.samples);

        let rms = (self.samples.iter().map(|s| s * s).sum::<f32>() / FFT_SIZE as f32).sqrt();
        let silent = rms < 1e-4;

        for ((c, s), w) in self.buf.iter_mut().zip(&self.samples).zip(&self.window) {
            *c = Complex::new(s * w, 0.0);
        }
        self.fft.process(&mut self.buf);

        let half = FFT_SIZE / 2 - 1;
        let mag = |k: usize| self.buf[k.clamp(1, half)].norm();
        for (i, &(lo, hi)) in self.bands.iter().enumerate() {
            let m = if hi - lo < 1.0 {
                // Banda más estrecha que un bin: se interpola.
                let c = (lo + hi) / 2.0;
                let k = c.floor() as usize;
                let t = c - c.floor();
                mag(k) * (1.0 - t) + mag(k + 1) * t
            } else {
                let (a, b) = (lo.ceil() as usize, (hi.floor() as usize).max(lo.ceil() as usize));
                (a..=b).map(mag).sum::<f32>() / (b - a + 1) as f32
            };
            self.raw[i] = if silent { 0.0 } else { (1.0 + m * self.weights[i]).ln() };
        }

        // Suavizado ligero entre barras vecinas.
        let n = self.raw.len();
        let mut targets: Vec<f32> = (0..n)
            .map(|i| {
                let l = self.raw[i.saturating_sub(1)];
                let r = self.raw[(i + 1).min(n - 1)];
                (0.25 * l + 0.5 * self.raw[i] + 0.25 * r) * self.sens
            })
            .collect();

        // Sensibilidad automática: que las barras usen casi toda la altura sin
        // quedarse pegadas arriba.
        let max = targets.iter().cloned().fold(0.0, f32::max);
        let frame = dt * 60.0;
        if max > 0.95 {
            self.sens *= 0.97f32.powf(frame);
        } else if !silent {
            self.sens *= 1.004f32.powf(frame);
        }
        self.sens = self.sens.clamp(1e-3, 50.0);
        for t in &mut targets {
            *t = t.min(1.0);
        }

        for i in 0..n {
            let (bar, target) = (&mut self.bars[i], targets[i]);
            if target > *bar {
                *bar += (target - *bar) * (1.0 - 0.35f32.powf(frame));
                self.velocity[i] = 0.0;
            } else {
                self.velocity[i] += 4.0 * dt;
                *bar = (*bar - self.velocity[i] * dt).max(target);
            }
            let peak = &mut self.peaks[i];
            *peak = (*peak - 0.4 * dt).max(*bar);
        }
    }
}
