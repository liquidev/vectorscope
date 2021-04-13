use std::sync::{Arc, Mutex};

use cpal::{
    Stream, StreamConfig, SampleRate, BufferSize,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use pixels::{Pixels, SurfaceTexture};
use winit::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{WindowBuilder},
};

struct AudioState {
    stream: Stream,
    samples_back: Arc<Mutex<Vec<f32>>>,
    samples_front: Vec<f32>,
}

impl AudioState {
    fn new() -> anyhow::Result<Self> {
        // find host
        let host_id = cpal::available_hosts()
            .into_iter()
            .find(|id| *id == cpal::HostId::Jack)
            .expect("could not find JACK host. make sure JACK is installed");
        let host = cpal::host_from_id(host_id)?;
        let input = host.default_input_device().unwrap();

        let arc_samples = Arc::new(Mutex::new(Vec::new()));

        let samples = arc_samples.clone();
        let stream = input.build_input_stream(
            &StreamConfig {
                channels: 2,
                sample_rate: SampleRate(48000),
                buffer_size: BufferSize::Fixed(256),
            },
            move |in_samples: &[f32], _info| {
                let mut samples = samples.lock().unwrap();
                samples.extend(in_samples.iter());
            },
            |error| {
                eprintln!("audio error: {}", error);
            },
        )?;
        stream.play()?;

        Ok(AudioState {
            stream,
            samples_back: arc_samples.clone(),
            samples_front: Vec::new(),
        })
    }

    fn render(&mut self, mut plot: Plot) {
        let (half_width, half_height) = (plot.width as f32 / 2.0, plot.height as f32 / 2.0);
        // flip buffers
        {
            const BUFFER_SIZE: usize = 2048;
            let mut samples = self.samples_back.lock().unwrap();
            if samples.len() > BUFFER_SIZE {
                self.samples_front.clear();
                self.samples_front.extend(samples.drain(..));
            }
        }
        // plot the samples
        plot.clear();
        for channels in self.samples_front.chunks_exact(2) {
            if let [left, right, ..] = channels {
                let x = left * half_width + half_width;
                let y = -right * half_height + half_height;
                plot.dot(x, y);
            }
        }
    }
}

struct Plot<'a> {
    width: u32,
    height: u32,
    pixels: &'a mut [u8]
}

impl Plot<'_> {
    fn clear(&mut self) {
        const DIVISIONS: u32 = 5;
        let division_width = self.width / DIVISIONS;
        let division_height = self.height / DIVISIONS;
        for x in 0..self.width {
            for y in 0..self.height {
                let i = ((x + y * self.width) * 4) as usize;
                let (r, g, b, a) = (i, i + 1, i + 2, i + 3);
                if (x + division_width / 2) % division_width == 0 || (y + division_height / 2) % division_height == 0 {
                    const BRIGHTNESS: u8 = 32;
                    self.pixels[r] = BRIGHTNESS;
                    self.pixels[g] = BRIGHTNESS;
                    self.pixels[b] = BRIGHTNESS;
                } else {
                    self.pixels[r] = 0;
                    self.pixels[g] = 0;
                    self.pixels[b] = 0;
                }
                self.pixels[a] = 255;
            }
        }
    }

    fn pixel(&mut self, x: f32, y: f32) {
        if (0.0..self.width as f32).contains(&x) && (0.0..self.height as f32).contains(&y) {
            let (x, y) = (x as u32, y as u32);
            let i = ((x + y * self.width) * 4) as usize;
            let (r, g, b, a) = (i, i + 1, i + 2, i + 3);
            self.pixels[g] = self.pixels[g].saturating_add(96);
            let intensity = ((self.pixels[g] as f32 - 192.0).max(0.0) * 2.0) as u8;
            self.pixels[r] = intensity;
            self.pixels[b] = intensity;
            self.pixels[a] = 255;
        }
    }

    fn dot(&mut self, x: f32, y: f32) {
        const R: i32 = 1;
        let (x, y) = (x as i32, y as i32);
        for yy in y - R..y + R {
            for xx in x - R..x + R {
                self.pixel(xx as f32, yy as f32);
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("vectorscope")
        .with_inner_size(LogicalSize::new(800, 800))
        .build(&event_loop)?;

    let window_size = window.inner_size();
    let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
    let mut pixels = Pixels::new(window_size.width, window_size.height, surface_texture)?;

    let mut state = AudioState::new()?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::WindowEvent { event, .. } => {
                match event {
                    WindowEvent::CloseRequested =>
                        *control_flow = ControlFlow::Exit,
                    WindowEvent::Resized { .. } => {
                        let window_size = window.inner_size();
                        pixels.resize(window_size.width, window_size.height);
                    },
                    _ => (),
                }
            },

            Event::MainEventsCleared => {
                window.request_redraw();
            },

            Event::RedrawRequested(_) => {
                state.render(Plot {
                    width: window_size.width,
                    height: window_size.height,
                    pixels: pixels.get_frame()
                });
                pixels.render().unwrap();
            },

            _ => (),
        }
    })
}
