use esp_hal::sync::Locked;

#[cfg_attr(any(esp32, esp32s2, esp32s3), path = "xtensa.rs")]
#[cfg_attr(any(esp32c2, esp32c3, esp32c6, esp32h2), path = "riscv.rs")]
mod arch_specific;

pub(crate) use arch_specific::*;

use crate::TimeBase;

pub(crate) static TIMER: Locked<Option<TimeBase>> = Locked::new(None);
