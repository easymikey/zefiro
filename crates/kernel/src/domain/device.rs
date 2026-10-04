use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceName(String);

impl DeviceName {
    pub fn new(name: String) -> Result<Self, DeviceNameError> {
        if name.is_empty() {
            return Err(DeviceNameError::Empty);
        }
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeviceNameError {
    #[error("enter a device name")]
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDefault {
    Default,
    Named,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum OutputDevice {
    #[default]
    SystemDefault,
    Named(DeviceName),
}

impl OutputDevice {
    #[must_use]
    pub fn named(&self) -> Option<&DeviceName> {
        match self {
            Self::SystemDefault => None,
            Self::Named(name) => Some(name),
        }
    }
}

impl fmt::Display for OutputDevice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SystemDefault => formatter.write_str("default"),
            Self::Named(name) => name.fmt(formatter),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedDevice {
    pub name: DeviceName,
    pub default: DeviceDefault,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::device::{DeviceName, DeviceNameError};

    #[rstest]
    #[case::empty("".to_string(), Err(DeviceNameError::Empty))]
    #[case::named("Speakers".to_string(), Ok(()))]
    fn a_device_name_is_never_empty(
        #[case] name: String,
        #[case] expected: Result<(), DeviceNameError>,
    ) {
        assert_eq!(DeviceName::new(name).map(|_| ()), expected);
    }
}
