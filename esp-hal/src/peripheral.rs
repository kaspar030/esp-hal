//! # Exclusive peripheral access

use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

/// An exclusive reference to a peripheral.
///
/// This is functionally the same as a `&'a mut T`. The reason for having a
/// dedicated struct is memory efficiency:
///
/// Peripheral singletons are typically either zero-sized (for concrete
/// peripherals like `SPI2` or `UART0`) or very small (for example `AnyPin`
/// which is 1 byte). However `&mut T` is always 4 bytes for 32-bit targets,
/// even if T is zero-sized. PeripheralRef stores a copy of `T` instead, so it's
/// the same size.
///
/// but it is the size of `T` not the size
/// of a pointer. This is useful if T is a zero sized type.
pub struct PeripheralRef<'a, T> {
    inner: T,
    _lifetime: PhantomData<&'a mut T>,
}

impl<'a, T> PeripheralRef<'a, T> {
    /// Create a new exclusive reference to a peripheral
    #[inline]
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            _lifetime: PhantomData,
        }
    }

    /// Unsafely clone (duplicate) a peripheral singleton.
    ///
    /// # Safety
    ///
    /// This returns an owned clone of the peripheral. You must manually ensure
    /// only one copy of the peripheral is in use at a time. For example, don't
    /// create two SPI drivers on `SPI1`, because they will "fight" each other.
    ///
    /// You should strongly prefer using `reborrow()` instead. It returns a
    /// `PeripheralRef` that borrows `self`, which allows the borrow checker
    /// to enforce this at compile time.
    pub unsafe fn clone_unchecked(&mut self) -> PeripheralRef<'a, T>
    where
        T: Peripheral<P = T>,
    {
        PeripheralRef::new(self.inner.clone_unchecked())
    }

    /// Reborrow into a "child" PeripheralRef.
    ///
    /// `self` will stay borrowed until the child PeripheralRef is dropped.
    pub fn reborrow(&mut self) -> PeripheralRef<'_, T>
    where
        T: Peripheral<P = T>,
    {
        // safety: we're returning the clone inside a new PeripheralRef that borrows
        // self, so user code can't use both at the same time.
        PeripheralRef::new(unsafe { self.inner.clone_unchecked() })
    }
}

impl<'a, T> Deref for PeripheralRef<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, T> DerefMut for PeripheralRef<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Trait for any type that can be used as a peripheral of type `P`.
///
/// This is used in driver constructors, to allow passing either owned
/// peripherals (e.g. `UART0`), or borrowed peripherals (e.g. `&mut UART0`).
///
/// For example, if you have a driver with a constructor like this:
///
/// ```rust, ignore
/// impl<'d, T> Uart<'d, T, Blocking> {
///     pub fn new<TX: PeripheralOutput, RX: PeripheralInput>(
///         uart: impl Peripheral<P = T> + 'd,
///         rx: impl Peripheral<P = RX> + 'd,
///         tx: impl Peripheral<P = TX> + 'd,
///     ) -> Result<Self, Error> {
///         Ok(Self { .. })
///     }
/// }
/// ```
///
/// You may call it with owned peripherals, which yields an instance that can
/// live forever (`'static`):
///
/// ```rust, ignore
/// let mut uart: Uart<'static, ...> = Uart::new(p.UART0, pins.gpio0, pins.gpio1);
/// ```
///
/// Or you may call it with borrowed peripherals, which yields an instance that
/// can only live for as long as the borrows last:
///
/// ```rust, ignore
/// let mut uart: Uart<'_, ...> = Uart::new(&mut p.UART0, &mut pins.gpio0, &mut pins.gpio1);
/// ```
///
/// # Implementation details, for HAL authors
///
/// When writing a HAL, the intended way to use this trait is to take `impl
/// Peripheral<P = ..>` in the HAL's public API (such as driver constructors),
/// calling `.into_ref()` to obtain a `PeripheralRef`, and storing that in the
/// driver struct.
///
/// `.into_ref()` on an owned `T` yields a `PeripheralRef<'static, T>`.
/// `.into_ref()` on an `&'a mut T` yields a `PeripheralRef<'a, T>`.
pub trait Peripheral: Sized + crate::private::Sealed {
    /// Peripheral singleton type
    type P;

    /// Unsafely clone (duplicate) a peripheral singleton.
    ///
    /// # Safety
    ///
    /// This returns an owned clone of the peripheral. You must manually ensure
    /// only one copy of the peripheral is in use at a time. For example, don't
    /// create two SPI drivers on `SPI1`, because they will "fight" each other.
    ///
    /// You should strongly prefer using `into_ref()` instead. It returns a
    /// `PeripheralRef`, which allows the borrow checker to enforce this at
    /// compile time.
    unsafe fn clone_unchecked(&mut self) -> Self::P;

    /// Convert a value into a `PeripheralRef`.
    ///
    /// When called on an owned `T`, yields a `PeripheralRef<'static, T>`.
    /// When called on an `&'a mut T`, yields a `PeripheralRef<'a, T>`.
    #[inline]
    fn into_ref<'a>(mut self) -> PeripheralRef<'a, Self::P>
    where
        Self: 'a,
    {
        PeripheralRef::new(unsafe { self.clone_unchecked() })
    }
}

impl<T, P> Peripheral for &mut T
where
    T: Peripheral<P = P>,
{
    type P = P;

    unsafe fn clone_unchecked(&mut self) -> Self::P {
        T::clone_unchecked(self)
    }
}

impl<T> crate::private::Sealed for &mut T where T: crate::private::Sealed {}

mod peripheral_macros {
    #[doc(hidden)]
    #[macro_export]
    macro_rules! peripherals {
        ($($(#[$cfg:meta])? $name:ident <= $from_pac:tt $(($($interrupt:ident),*))? ),*$(,)?) => {

            /// Contains the generated peripherals which implement [`Peripheral`]
            mod peripherals {
                pub use super::pac::*;
                $(
                    $crate::create_peripheral!($(#[$cfg])? $name <= $from_pac);
                )*
            }

            /// The `Peripherals` struct provides access to all of the hardware peripherals on the chip.
            #[allow(non_snake_case)]
            pub struct Peripherals {
                $(
                    $(#[$cfg])?
                    /// Each field represents a hardware peripheral.
                    pub $name: peripherals::$name,
                )*
            }

            impl Peripherals {
                /// Returns all the peripherals *once*
                #[inline]
                pub(crate) fn take() -> Self {
                    #[no_mangle]
                    static mut _ESP_HAL_DEVICE_PERIPHERALS: bool = false;

                    critical_section::with(|_| unsafe {
                        if _ESP_HAL_DEVICE_PERIPHERALS {
                            panic!("init called more than once!")
                        }
                        _ESP_HAL_DEVICE_PERIPHERALS = true;
                        Self::steal()
                    })
                }
            }

            impl Peripherals {
                /// Unsafely create an instance of this peripheral out of thin air.
                ///
                /// # Safety
                ///
                /// You must ensure that you're only using one instance of this type at a time.
                #[inline]
                pub unsafe fn steal() -> Self {
                    Self {
                        $(
                            $(#[$cfg])?
                            $name: peripherals::$name::steal(),
                        )*
                    }
                }
            }

            #[allow(non_snake_case)]
            pub struct OptionalPeripherals {
                $(
                    $(#[$cfg])?
                    pub $name: Option<peripherals::$name>,
                )*
                // These GPIO peripherals are intended to be populated later, when the `Io` type is
                // instantiated, initializing GPIOs.
                // We need to define them here so that users can access them like every other
                // peripherals.
                pub GPIO_0: Option<crate::gpio::GPIO_0>,
                pub GPIO_1: Option<crate::gpio::GPIO_1>,
                pub GPIO_2: Option<crate::gpio::GPIO_2>,
                pub GPIO_3: Option<crate::gpio::GPIO_3>,
                pub GPIO_4: Option<crate::gpio::GPIO_4>,
                pub GPIO_5: Option<crate::gpio::GPIO_5>,
                pub GPIO_6: Option<crate::gpio::GPIO_6>,
                pub GPIO_7: Option<crate::gpio::GPIO_7>,
                pub GPIO_8: Option<crate::gpio::GPIO_8>,
                pub GPIO_9: Option<crate::gpio::GPIO_9>,
                pub GPIO_10: Option<crate::gpio::GPIO_10>,
                pub GPIO_11: Option<crate::gpio::GPIO_11>,
                pub GPIO_12: Option<crate::gpio::GPIO_12>,
                pub GPIO_13: Option<crate::gpio::GPIO_13>,
                pub GPIO_14: Option<crate::gpio::GPIO_14>,
                pub GPIO_15: Option<crate::gpio::GPIO_15>,
                pub GPIO_16: Option<crate::gpio::GPIO_16>,
                pub GPIO_17: Option<crate::gpio::GPIO_17>,
                pub GPIO_18: Option<crate::gpio::GPIO_18>,
                pub GPIO_19: Option<crate::gpio::GPIO_19>,
                pub GPIO_20: Option<crate::gpio::GPIO_20>,
                pub GPIO_21: Option<crate::gpio::GPIO_21>,
                pub GPIO_22: Option<crate::gpio::GPIO_22>,
                pub GPIO_23: Option<crate::gpio::GPIO_23>,
                pub GPIO_24: Option<crate::gpio::GPIO_24>,
                pub GPIO_25: Option<crate::gpio::GPIO_25>,
                pub GPIO_26: Option<crate::gpio::GPIO_26>,
                pub GPIO_27: Option<crate::gpio::GPIO_27>,
                pub GPIO_28: Option<crate::gpio::GPIO_28>,
                pub GPIO_29: Option<crate::gpio::GPIO_29>,
                pub GPIO_30: Option<crate::gpio::GPIO_30>,
            }

            impl OptionalPeripherals {
                /// Create an `OptionalPeripherals`, consuming a `Peripherals`
                #[inline]
                pub fn from(p: Peripherals) -> Self {
                    Self {
                        $(
                            $(#[$cfg])?
                            $name: Some(p.$name),
                        )*
                        GPIO_0: None,
                        GPIO_1: None,
                        GPIO_2: None,
                        GPIO_3: None,
                        GPIO_4: None,
                        GPIO_5: None,
                        GPIO_6: None,
                        GPIO_7: None,
                        GPIO_8: None,
                        GPIO_9: None,
                        GPIO_10: None,
                        GPIO_11: None,
                        GPIO_12: None,
                        GPIO_13: None,
                        GPIO_14: None,
                        GPIO_15: None,
                        GPIO_16: None,
                        GPIO_17: None,
                        GPIO_18: None,
                        GPIO_19: None,
                        GPIO_20: None,
                        GPIO_21: None,
                        GPIO_22: None,
                        GPIO_23: None,
                        GPIO_24: None,
                        GPIO_25: None,
                        GPIO_26: None,
                        GPIO_27: None,
                        GPIO_28: None,
                        GPIO_29: None,
                        GPIO_30: None,
                    }
                }
            }

            // expose the new structs
            $(
                pub use peripherals::$name;
            )*

            $(
                $(
                    impl peripherals::$name {
                        $(
                            paste::paste!{
                                /// Binds an interrupt handler to the corresponding interrupt for this peripheral.
                                pub fn [<bind_ $interrupt:lower _interrupt >](&mut self, handler: unsafe extern "C" fn() -> ()) {
                                    unsafe { $crate::interrupt::bind_interrupt($crate::peripherals::Interrupt::$interrupt, handler); }
                                }
                            }
                        )*
                    }
                )*
            )*

        }
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! into_ref {
        ($($name:ident),*) => {
            $(
                #[allow(unused_mut)]
                let mut $name = $name.into_ref();
            )*
        }
    }

    #[doc(hidden)]
    #[macro_export]
    /// Macro to create a peripheral structure.
    macro_rules! create_peripheral {
        ($(#[$cfg:meta])? $name:ident <= virtual) => {
            $(#[$cfg])?
            #[derive(Debug)]
            #[allow(non_camel_case_types)]
            /// Represents a virtual peripheral with no associated hardware.
            ///
            /// This struct is generated by the `create_peripheral!` macro when the peripheral
            /// is defined as virtual.
            pub struct $name { _inner: () }

            $(#[$cfg])?
            impl $name {
                /// Unsafely create an instance of this peripheral out of thin air.
                ///
                /// # Safety
                ///
                /// You must ensure that you're only using one instance of this type at a time.
                #[inline]
                pub unsafe fn steal() -> Self {
                    Self { _inner: () }
                }
            }

            impl $crate::peripheral::Peripheral for $name {
                type P = $name;

                #[inline]
                unsafe fn clone_unchecked(&mut self) -> Self::P {
                    Self::steal()
                }
            }

            impl $crate::private::Sealed for $name {}
        };
        ($(#[$cfg:meta])? $name:ident <= $base:ident) => {
            $(#[$cfg])?
            #[derive(Debug)]
            #[allow(non_camel_case_types)]
            /// Represents a concrete hardware peripheral.
            ///
            /// This struct is generated by the `create_peripheral!` macro when the peripheral
            /// is tied to an actual hardware device.
            pub struct $name { _inner: () }

            $(#[$cfg])?
            impl $name {
                /// Unsafely create an instance of this peripheral out of thin air.
                ///
                /// # Safety
                ///
                /// You must ensure that you're only using one instance of this type at a time.
                #[inline]
                pub unsafe fn steal() -> Self {
                    Self { _inner: () }
                }

                #[doc = r"Pointer to the register block"]
                pub const PTR: *const <super::pac::$base as core::ops::Deref>::Target = super::pac::$base::PTR;

                #[doc = r"Return the pointer to the register block"]
                #[inline(always)]
                pub const fn ptr() -> *const <super::pac::$base as core::ops::Deref>::Target {
                    super::pac::$base::PTR
                }
            }

            impl core::ops::Deref for $name {
                type Target = <super::pac::$base as core::ops::Deref>::Target;

                fn deref(&self) -> &Self::Target {
                    unsafe { &*Self::PTR }
                }
            }

            impl core::ops::DerefMut for $name {

                fn deref_mut(&mut self) -> &mut Self::Target {
                    unsafe { &mut *(Self::PTR as *mut _)  }
                }
            }

            impl $crate::peripheral::Peripheral for $name {
                type P = $name;

                #[inline]
                unsafe fn clone_unchecked(&mut self) -> Self::P {
                    Self::steal()
                }
            }

            impl $crate::private::Sealed for $name {}
        };
    }
}
