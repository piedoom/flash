#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::convert::Infallible;
use embedded_nrf24l01 as nrf;
use hal::{
    gpio::{gpiob::*, Alternate, Floating, Input, Output, PushPull},
    prelude::*,
};
use rtic::cyccnt::{U32Ext as _};
use rtic::app;
use stm32f1::stm32f103::SPI2;
use stm32f1xx_hal as hal;
use cortex_m_semihosting::hprintln;
use hal::{
    spi::{Spi, Spi2NoRemap},
    time::MegaHertz,
};

use nrf::{Configuration, CrcMode, DataRate, StandbyMode, NRF24L01};

type RadioCe = PB0<Output<PushPull>>;
type RadioCsn = PB1<Output<PushPull>>;
type RadioSpi2Pins = (
    PB13<Alternate<PushPull>>,
    PB14<Input<Floating>>,
    PB15<Alternate<PushPull>>,
);
type RadioSpi = Spi<SPI2, Spi2NoRemap, RadioSpi2Pins>;
type Radio = NRF24L01<Infallible, RadioCe, RadioCsn, RadioSpi>;

// From https://github.com/davidji/rust-rc
pub const TX_ADDRESS: [u8; 5] = ['R' as u8, 'C' as u8, 'T' as u8, 'X' as u8, 0x00];
pub const RX_ADDRESS: [u8; 5] = ['R' as u8, 'C' as u8, 'R' as u8, 'X' as u8, 0x00];
pub const BUFFER_SIZE: usize = 32;

const FREQ: u32 = 48;
const SYSCLK_FREQ: MegaHertz = MegaHertz(FREQ);
const PCLK1_FREQ: MegaHertz = MegaHertz(FREQ / 2);

#[app(device = stm32f1xx_hal::pac, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        radio: Option<StandbyMode<Radio>>,
        buffer: Option<[u8; BUFFER_SIZE]>,
    }
    #[init(spawn = [transmit])]
    fn init(cx: init::Context) -> init::LateResources {
        hprintln!("Initializing device!").unwrap();
        // Enable the monotonic timer
        let mut core = cx.core;
        core.DWT.enable_cycle_counter();
        let _ = cx.start;

        hprintln!("Setting up peripherals!").unwrap();
        let mut rcc = cx.device.RCC.constrain();
        let mut flash = cx.device.FLASH.constrain();
        let _mapr = cx.device.AFIO.constrain(&mut rcc.apb2).mapr;
        let clocks = rcc
            .cfgr
            .sysclk(SYSCLK_FREQ)
            .pclk1(PCLK1_FREQ)
            .freeze(&mut flash.acr);

        hprintln!("Setting up SPI!").unwrap();
        let mut gpiob = cx.device.GPIOB.split(&mut rcc.apb2);

        let spi_pins: RadioSpi2Pins = (
            gpiob.pb13.into_alternate_push_pull(&mut gpiob.crh),
            gpiob.pb14.into_floating_input(&mut gpiob.crh),
            gpiob.pb15.into_alternate_push_pull(&mut gpiob.crh),
        );

        // Pins for the wireless transmitter
        let (ce, csn): (RadioCe, RadioCsn) = (
            gpiob.pb0.into_push_pull_output(&mut gpiob.crl),
            gpiob.pb1.into_push_pull_output(&mut gpiob.crl),
        );

        let spi: RadioSpi = Spi::spi2(
            cx.device.SPI2,
            spi_pins,
            nrf::setup::spi_mode(),
            nrf::setup::clock_mhz().mhz(),
            clocks,
            &mut rcc.apb1,
        );

        hprintln!("Setting up the radio!").unwrap();
        let mut radio: StandbyMode<Radio> = NRF24L01::new(ce, csn, spi).expect("to create a new radio interface");

        radio.set_frequency(FREQ as u8).expect("to set frequency");
        radio.set_rx_addr(0, &RX_ADDRESS).expect("to set address");
        radio.set_tx_addr(&RX_ADDRESS).expect("to set tx address");
        radio.set_rf(DataRate::R250Kbps, 0).expect("to set frequency");
        radio.set_auto_retransmit(0b0100, 15).expect("to set retransmit");
        radio.set_crc(Some(CrcMode::TwoBytes)).expect("to set crc mode");
        radio.set_pipes_rx_lengths(&[None; 6]).expect("to set pipes length");
        radio.flush_tx().expect("to flush tx");
        radio.flush_rx().expect("to flush rx");

        hprintln!("Beginning transmissions!").unwrap();
        cx.spawn.transmit().expect("to schedule a transmission");
        
        init::LateResources {
            radio: Some(radio),
            buffer: Some([0u8; 32]),
        }
    }

    #[task(resources = [radio, buffer], schedule = [transmit])]
    fn transmit(cx: transmit::Context) {
        hprintln!("Attempting to send message.").unwrap();
        let message = b"message";

        let mut standby = cx.resources.radio.take().unwrap();
        let mut buffer = cx.resources.buffer.take().unwrap();

        // modify our buffer to include our message
        for (i, letter) in message.iter().enumerate() {
            buffer[i] = *letter;
        }
        
        // Get the device ready to transmit
        standby.flush_tx().unwrap();
        standby.flush_rx().unwrap();
        let mut tx = standby.tx().unwrap();

        // We can send a maximum of 32 bytes per packet with the NRF24L01
        // this isnt sending our message...
        if tx.can_send().unwrap() {
            tx.send(&buffer).unwrap();
            match tx.wait_empty() {
                Ok(_) => { hprintln!("transmitted!").unwrap(); },
                Err(_) => { hprintln!("error transmitting").unwrap(); } // If we can't transmit this time, perhaps we can next time...
            }
        } else {
            hprintln!("Not ready to transmit.").unwrap();
        }

        // Clear the buffer
        buffer = [0u8; 32];
        
        // Give back ownership of the radio and buffer, and schedule another loop
        *cx.resources.radio = Some(tx.standby().unwrap());
        *cx.resources.buffer = Some(buffer);
        cx.schedule.transmit(cx.scheduled + (FREQ * 1_000_000).cycles()).unwrap();

    }

    extern "C" {
        fn EXTI0();
    }
};
