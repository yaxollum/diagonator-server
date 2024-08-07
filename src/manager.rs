use crate::config::{LockedTimeRangeConfig, RequirementConfig};
use crate::server::Response;
use crate::simulator::{Simulator, StateChange, StateChangeKind};
use crate::time::{Duration, HourMinute, LocalDate, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Requirement {
    id: u64,
    name: String,
    due: Timestamp,
    complete: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct TimeRange {
    id: u64,
    start: Option<Timestamp>,
    end: Option<Timestamp>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
enum BreakTimer {
    Unlocked { until: Timestamp },
    Locked { until: Timestamp },
    Unlockable,
}

struct BreakTimerManager {
    timer: BreakTimer,
    work_period_duration: Duration,
    break_duration: Duration,
}

impl BreakTimerManager {
    fn new(work_period_duration: Duration, break_duration: Duration) -> Self {
        Self {
            timer: BreakTimer::Unlockable,
            work_period_duration,
            break_duration,
        }
    }
    fn unlock(&mut self, current_time: Timestamp) -> Result<(), String> {
        self.refresh(current_time);
        match self.timer {
            BreakTimer::Unlockable => {
                self.timer = BreakTimer::Unlocked {
                    until: current_time + self.work_period_duration,
                };
                Ok(())
            }
            BreakTimer::Locked { until: _ } => Err("Break timer is locked.".to_owned()),
            BreakTimer::Unlocked { until: _ } => Err("Break timer is already unlocked.".to_owned()),
        }
    }
    fn lock(&mut self, current_time: Timestamp) -> Result<(), String> {
        self.refresh(current_time);
        match self.timer {
            BreakTimer::Unlocked { until: _ } => {
                self.timer = BreakTimer::Locked {
                    until: current_time + self.break_duration,
                };
                Ok(())
            }
            _ => Err("Break timer is not unlocked.".to_owned()),
        }
    }
    fn refresh(&mut self, current_time: Timestamp) {
        if let BreakTimer::Unlocked { until } = self.timer {
            if current_time >= until {
                self.timer = BreakTimer::Locked {
                    until: until + self.break_duration,
                };
            }
        }
        if let BreakTimer::Locked { until } = self.timer {
            if current_time >= until {
                self.timer = BreakTimer::Unlockable;
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentState {
    Unlocked,
    Locked,
    Unlockable,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum CurrentStateReason {
    BreakTimer,
    RequirementNotMet { id: u64 },
    LockedTimeRange { id: u64 },
    NoConstraints,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CurrentInfo {
    state: CurrentState,
    until: Option<Timestamp>,
    reason: CurrentStateReason,
    locked_time_ranges: Vec<TimeRange>,
    requirements: Vec<Requirement>,
    deactivated_until: Option<Timestamp>,
    diagonator_running: bool,
}
struct Constraints {
    break_timer: BreakTimerManager,
    requirements: Vec<Requirement>,
    locked_time_ranges: Vec<TimeRange>,
    deactivated_until: Option<Timestamp>,
}

impl Constraints {
    fn get_current_info(&mut self, current_time: Timestamp) -> CurrentInfo {
        self.break_timer.refresh(current_time);
        if let Some(du) = self.deactivated_until {
            if current_time >= du {
                self.deactivated_until = None;
            }
        }
        let mut simulator = Simulator::new();
        // now we push the state changes into the simulator in the following order:
        // 1. requirements
        // 2. locked time ranges
        // 3. break timer
        // this ensures that if multiple state changes occur at the same time,
        // requirements and locked time ranges will get first and second priority,
        // respectively, when determining the reason
        for requirement in &self.requirements {
            if !requirement.complete {
                simulator.push(StateChange {
                    kind: StateChangeKind::RequirementLocked(requirement.id),
                    time: requirement.due,
                })
            }
        }
        for ltr in &self.locked_time_ranges {
            simulator.push(StateChange {
                kind: StateChangeKind::RangeLocked(ltr.id),
                time: ltr.start.unwrap_or(Timestamp::ZERO),
            });
            if let Some(ltr_end) = ltr.end {
                simulator.push(StateChange {
                    kind: StateChangeKind::RangeUnlocked(ltr.id),
                    time: ltr_end,
                })
            }
        }
        match &self.break_timer.timer {
            BreakTimer::Unlocked { until } => simulator.push(StateChange {
                kind: StateChangeKind::BreakTimerLocked,
                time: *until,
            }),
            BreakTimer::Locked { until } => {
                simulator.push(StateChange {
                    kind: StateChangeKind::BreakTimerLocked,
                    time: Timestamp::ZERO,
                });
                simulator.push(StateChange {
                    kind: StateChangeKind::BreakTimerUnlockable,
                    time: *until,
                });
            }
            BreakTimer::Unlockable => simulator.push(StateChange {
                kind: StateChangeKind::BreakTimerUnlockable,
                time: Timestamp::ZERO,
            }),
        }
        let result = simulator.run(current_time);
        let diagonator_running = !(matches!(result.target_state, CurrentState::Unlocked)
            || self.deactivated_until.is_some());
        CurrentInfo {
            state: result.target_state,
            until: result.until,
            reason: result.reason,
            locked_time_ranges: self.locked_time_ranges.clone(),
            requirements: self.requirements.clone(),
            deactivated_until: self.deactivated_until,
            diagonator_running,
        }
    }
    fn complete_requirement(&mut self, id: u64) -> Result<(), String> {
        for req in &mut self.requirements {
            if req.id == id {
                if !req.complete {
                    req.complete = true;
                    return Ok(());
                } else {
                    return Err(format!("Requirement {} has already been completed.", id));
                }
            }
        }
        Err(format!("Requirement {} not found.", id))
    }
}

pub struct DiagonatorManager {
    manager: DiagonatorManagerInner,
    cached_info: CurrentInfo,
    cache_time: Timestamp,
    cache_version: u64,
}

impl DiagonatorManager {
    pub const NO_CACHE: u64 = 0;
    pub fn new(config: DiagonatorManagerConfig, current_time: Timestamp) -> Self {
        let mut manager = DiagonatorManagerInner::new(config);
        let cached_info = manager.refresh(current_time);
        Self {
            manager,
            cached_info,
            cache_time: current_time,
            cache_version: Self::NO_CACHE + 1,
        }
    }
    pub fn unlock_timer(&mut self, current_time: Timestamp) -> Response {
        let info = self.refresh_cache(current_time);
        if matches!(info.state, CurrentState::Unlockable) {
            match self.manager.constraints.break_timer.unlock(current_time) {
                Ok(()) => {
                    self.refresh_cache(current_time);
                    Response::Success
                }
                Err(msg) => Response::Error { msg },
            }
        } else {
            Response::Error {
                msg: "Session is not unlockable.".to_owned(),
            }
        }
    }
    pub fn lock_timer(&mut self, current_time: Timestamp) -> Response {
        self.manager.constraints.deactivated_until = None;
        self.refresh_cache(current_time);
        match self.manager.constraints.break_timer.lock(current_time) {
            Ok(()) => {
                self.refresh_cache(current_time);
                Response::Success
            }
            Err(msg) => Response::Error { msg },
        }
    }
    pub fn get_info(&self) -> CurrentInfo {
        self.cached_info.clone()
    }
    pub fn get_info_if_changed(
        &mut self,
        cache_version: u64,
        current_time: Timestamp,
    ) -> Option<(CurrentInfo, u64)> {
        if current_time != self.cache_time {
            self.refresh_cache(current_time);
        }
        if cache_version != self.cache_version {
            Some((self.cached_info.clone(), self.cache_version))
        } else {
            None
        }
    }
    pub fn get_info_once(&mut self, current_time: Timestamp) -> Response {
        Response::Info {
            info: self.refresh_cache(current_time),
        }
    }
    pub fn complete_requirement(
        &mut self,
        current_time: Timestamp,
        requirement_id: u64,
    ) -> Response {
        self.refresh_cache(current_time);
        match self
            .manager
            .constraints
            .complete_requirement(requirement_id)
        {
            Ok(()) => {
                self.refresh_cache(current_time);
                Response::Success
            }
            Err(msg) => Response::Error { msg },
        }
    }
    pub fn add_requirement(
        &mut self,
        current_time: Timestamp,
        name: String,
        due: HourMinute,
    ) -> Response {
        self.refresh_cache(current_time);
        self.manager.constraints.requirements.push(Requirement {
            id: self.manager.id_generator.next_id(),
            name,
            due: Timestamp::from_date_hm(&self.manager.current_date, &due),
            complete: false,
        });
        self.refresh_cache(current_time);
        Response::Success
    }
    pub fn deactivate(&mut self, current_time: Timestamp, duration: Duration) -> Response {
        self.manager.constraints.deactivated_until = Some(current_time + duration);
        self.refresh_cache(current_time);
        Response::Success
    }
    fn refresh_cache(&mut self, current_time: Timestamp) -> CurrentInfo {
        self.cache_time = current_time;
        let new_info = self.manager.refresh(current_time);
        if new_info != self.cached_info {
            self.cached_info = new_info.clone();
            self.cache_version += 1;
        }
        new_info
    }
}

struct DiagonatorManagerInner {
    config: DiagonatorManagerConfig,
    constraints: Constraints,
    current_date: LocalDate,
    id_generator: IdGenerator,
}

impl DiagonatorManagerInner {
    pub fn new(config: DiagonatorManagerConfig) -> Self {
        let break_timer =
            BreakTimerManager::new(config.work_period_duration, config.break_duration);
        Self {
            config,
            constraints: Constraints {
                break_timer,
                requirements: Vec::new(),
                locked_time_ranges: Vec::new(),
                deactivated_until: None,
            },
            current_date: Timestamp::ZERO.get_date(),
            id_generator: IdGenerator::new(),
        }
    }
    fn new_day(&mut self) {
        self.constraints.requirements = self
            .config
            .requirements
            .iter()
            .map(|req| Requirement {
                id: self.id_generator.next_id(),
                name: req.name.clone(),
                due: Timestamp::from_date_hm(&self.current_date, &req.due),
                complete: false,
            })
            .collect();
        self.constraints.locked_time_ranges = self
            .config
            .locked_time_ranges
            .iter()
            .map(|ltr| TimeRange {
                id: self.id_generator.next_id(),
                start: Timestamp::from_date_hm_opt(&self.current_date, &ltr.start),
                end: Timestamp::from_date_hm_opt(&self.current_date, &ltr.end),
            })
            .collect();
    }
    fn refresh(&mut self, current_time: Timestamp) -> CurrentInfo {
        let current_date = current_time.get_date();
        if current_date != self.current_date {
            self.current_date = current_date;
            self.new_day();
        }
        let mut current_info = self.constraints.get_current_info(current_time);

        if current_info.diagonator_running {
            // if the break timer is unlocked, then we lock it and refresh the constraints
            if let Ok(()) = self.constraints.break_timer.lock(current_time) {
                current_info = self.constraints.get_current_info(current_time);
            }
        }
        current_info
    }
}

pub struct DiagonatorManagerConfig {
    pub requirements: Vec<RequirementConfig>,
    pub locked_time_ranges: Vec<LockedTimeRangeConfig>,
    pub work_period_duration: Duration,
    pub break_duration: Duration,
}

struct IdGenerator {
    last_id: u64,
}

impl IdGenerator {
    fn next_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }
    fn new() -> Self {
        Self { last_id: 0 }
    }
}
