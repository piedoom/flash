// Copyright (c) 2016 Josh Robson Chase 
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions: The above copyright
// notice and this permission notice shall be included in all copies or
// substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS",
// WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
// FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR
// THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use crate::LED_COUNT;
use smart_leds::RGB8;
use stm32f1xx_futures::hal::{
    prelude::*,
    pac::{SPI1},
    spi::{
        SpiTxDma,
    },
    dma::{
        dma1::C3
    },
    gpio::{gpioa::{PA5, PA6, PA7}, Input, PushPull, Alternate, Floating},
};
use cortex_m::singleton;
use as_slice::AsSlice;

/// Trait for a struct that can drive an RGB led strip
pub trait RgbDriver {
    /// Prepare to set the colour of the specified LED
    fn prepare_color(&mut self, index: usize, color: RGB8);
    /// Transmit all the configured colours
    fn transmit(&mut self);
}


type Pins = (
    PA5<Alternate<PushPull>>,
    PA6<Input<Floating>>,
    PA7<Alternate<PushPull>>
);

/**
  Poor man's const generics ;)
*/
macro_rules! spi_bit_container {
    ($name:ident, $led_amount:expr) => {
        struct $name {
            pub data: [u8; led_spi_bit_amount($led_amount)],
        }

        impl $name {
            pub fn new() -> Self {
                Self {
                    data: [0x0; led_spi_bit_amount($led_amount)],
                }
            }
        }

        const fn led_spi_bit_amount(led_amount: usize) -> usize{
            // Each led needs 24 bits transfered At 3 MHz, one pulse is 0.33333
            // us The pattern for a 0 is
            // ^^^^^^^|________________
            // | 0.35 |      0.9      | And for a 1
            // ^^^^^^^^^^^^^^^^|_______
            // |               | 0.35 | That is, a 1 is 4 high pulses, followed
            // by 1 low pulse And a 0 is 1 low pulse followed by 4 low

            // The delay between bits can be fairly long, so for simplicity, one
            // transfered byte will be used per bit

            // This means that the total length of the transmitted data is
            // 24*led_amount+reset time

            // Reset time is specified as 50 us which requires ~150 pulses. This
            // is ~160/8=20 bits
            let reset_count = 20;
            led_amount * 24 + reset_count
        }
    }
}

spi_bit_container!(RgbBitContainer, LED_COUNT);

impl AsSlice for RgbBitContainer {
    type Element = u8;
    fn as_slice(&self) -> &[u8] {
        &self.data
    }
}

pub struct Ws2812Driver {
    spi: Option<SpiTxDma<SPI1, Pins, C3>>,
    led_data: [RGB8; LED_COUNT],
    bit_storage: Option<&'static mut RgbBitContainer>
}

impl Ws2812Driver {
    pub fn new(spi: SpiTxDma<SPI1, Pins, C3>) -> Self {
        Self {
            spi: Some(spi),
            led_data: [RGB8::new(0, 0, 0); LED_COUNT],
            bit_storage: Some(
                singleton!(: RgbBitContainer = RgbBitContainer::new()).unwrap()
            )
        }
    }
}

impl RgbDriver for Ws2812Driver {
    fn prepare_color(&mut self, index: usize, color: RGB8) {
        // TODO: Handle out of bounds
        self.led_data[index] = color;
    }
    fn transmit(&mut self) {
        {
            let dma_ = self.spi.take().unwrap();
            let bits_ = self.bit_storage.take().unwrap();

            led_spi_bit_pattern(&self.led_data, &mut bits_.data);

            let transfer = dma_.write(bits_);
            let (bits_, dma_) = transfer.wait();

            self.spi.replace(dma_);
            self.bit_storage.replace(bits_);
        }
    }
}



pub fn led_spi_bit_pattern(
    leds: &[RGB8],
    mut output: &mut [u8]
) {
    // Set all LEDS to 0
    for i in 0..output.len() {
        output[i] = 0;
    }
    for led in leds {
        set_from_byte(led.g, output);
        output = &mut output[8..];
        set_from_byte(led.r, output);
        output = &mut output[8..];
        set_from_byte(led.b, output);
        output = &mut output[8..];
    }
}

fn set_from_byte(byte: u8, mut target: &mut [u8]) {
    for i in 0..8 {
        const MASK: u8 = 0b1000_0000;
        set_spi_byte((byte << i) & MASK == MASK, target);
        target = &mut target[1..]
    }
}

fn set_spi_byte(value: bool, target: &mut [u8]) {
    target[0] = match value {
        false => 0b10000000,
        true => 0b11110000,
    };
}

