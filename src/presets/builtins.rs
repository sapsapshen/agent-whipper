use crate::core::config::AgentState;
use crate::presets::manager::{Preset, PresetStep};

pub fn get_builtin_presets() -> Vec<Preset> {
    vec![dev_review_patch(), unblock(), speedup(), team_rescue()]
}

fn dev_review_patch() -> Preset {
    Preset {
        name: "dev-review-patch".to_string(),
        description: "开发→审核→修补循环".to_string(),
        trigger_on: vec![AgentState::Stalled],
        max_retries: 3,
        steps: vec![
            PresetStep::CtrlC,
            PresetStep::Wait { duration_secs: 0.5 },
            PresetStep::Text {
                content: vec!["/review".to_string()],
            },
            PresetStep::Enter,
            PresetStep::Wait { duration_secs: 2.0 },
            PresetStep::Text {
                content: vec!["审核以上代码，列出所有问题并逐一修复".to_string()],
            },
            PresetStep::Enter,
        ],
    }
}

fn unblock() -> Preset {
    Preset {
        name: "unblock".to_string(),
        description: "假死复苏".to_string(),
        trigger_on: vec![AgentState::Zombie],
        max_retries: 2,
        steps: vec![
            PresetStep::CtrlC,
            PresetStep::Wait { duration_secs: 1.0 },
            PresetStep::Text {
                content: vec![
                    "你卡住了。重新审视任务，从最简单路径重新开始。忽略之前的尝试。".to_string(),
                    "停下来，深呼吸。用最简单直接的方式完成任务。".to_string(),
                    "别死磕了，换个思路，直接给能工作的代码。".to_string(),
                ],
            },
            PresetStep::Enter,
        ],
    }
}

fn speedup() -> Preset {
    Preset {
        name: "speedup".to_string(),
        description: "加速".to_string(),
        trigger_on: vec![AgentState::Stalled],
        max_retries: 5,
        steps: vec![
            PresetStep::Text {
                content: vec![
                    "加快速度。不要过度思考，先给能工作的代码，再优化。跳过不必要的解释。"
                        .to_string(),
                    "别磨蹭了，先上能跑的代码，解释后面补上。".to_string(),
                    "加速加速！用最简洁的方式完成任务。".to_string(),
                ],
            },
            PresetStep::Enter,
        ],
    }
}

fn team_rescue() -> Preset {
    Preset {
        name: "team-rescue".to_string(),
        description: "oh-my-codex team 人工救援提示".to_string(),
        trigger_on: vec![],
        max_retries: 1,
        steps: vec![
            PresetStep::Text {
                content: vec![
                    "检测到团队协作可能卡住了。先确认准确的 omx team 名称，再手动执行恢复，不要对未知团队强制 shutdown。"
                        .to_string(),
                ],
            },
            PresetStep::Enter,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_presets_count() {
        let presets = get_builtin_presets();
        assert_eq!(presets.len(), 4);
    }

    #[test]
    fn test_dev_review_patch_triggers_on_stalled() {
        let preset = dev_review_patch();
        assert!(preset.trigger_on.contains(&AgentState::Stalled));
        assert!(!preset.trigger_on.contains(&AgentState::Zombie));
    }

    #[test]
    fn test_unblock_triggers_on_zombie() {
        let preset = unblock();
        assert!(preset.trigger_on.contains(&AgentState::Zombie));
        assert_eq!(preset.max_retries, 2);
    }

    #[test]
    fn test_speedup_high_retries() {
        let preset = speedup();
        assert_eq!(preset.max_retries, 5);
        assert_eq!(preset.steps.len(), 2);
    }

    #[test]
    fn test_team_rescue_is_manual_only() {
        let preset = team_rescue();
        assert_eq!(preset.max_retries, 1);
        assert!(preset.trigger_on.is_empty());
        assert_eq!(preset.steps.len(), 2);
    }

    #[test]
    fn test_unblock_has_multiple_messages() {
        let preset = unblock();
        for step in preset.steps {
            if let PresetStep::Text { content } = step {
                assert!(content.len() >= 3);
            }
        }
    }

    #[test]
    fn test_speedup_has_multiple_messages() {
        let preset = speedup();
        for step in preset.steps {
            if let PresetStep::Text { content } = step {
                assert!(content.len() >= 3);
            }
        }
    }
}
