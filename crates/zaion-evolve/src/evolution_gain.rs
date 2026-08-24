//! Net Evolution Gain 鈥?自进化净收益。
//!
//! 不只测"patch 是否成功"，而测"进化后是否真的比进化前更好"。
//!
//! Net Evolution Gain = Post-Evolution Capability
//!                        - Regression Cost
//!                        - Review Cost
//!                        - Runtime Cost
//!
//! Net gain >= 0 表示进化带来净收益；< 0 表示"越改越差"。

/// 自进化净收益的量化结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetEvolutionGain {
    /// 进化后的能力（例如通过测试数 + 新能力得分）。
    pub post_evolution_capability: i64,
    /// 回归成本（进化引入的测试失败/能力退化）。
    pub regression_cost: i64,
    /// 评审成本（Trinity review 轮次、被驳回的提案数）。
    pub review_cost: i64,
    /// 运行成本（LLM token、wall-clock 秒等归一化）。
    pub runtime_cost: i64,
}

impl NetEvolutionGain {
    /// 计算净收益。
    pub fn net_gain(&self) -> i64 {
        self.post_evolution_capability - self.regression_cost - self.review_cost - self.runtime_cost
    }

    /// 进化是否真的更好（净收益 >= 0）。
    pub fn is_net_positive(&self) -> bool {
        self.net_gain() >= 0
    }

    /// 人类可读的结论。
    pub fn verdict(&self) -> &'static str {
        if self.net_gain() > 0 {
            "improved"
        } else if self.net_gain() == 0 {
            "neutral"
        } else {
            "regressed"
        }
    }
}

/// 从一个 patch 应用的度量（能力/回归/评审/运行）计算净收益。
pub fn compute_net_gain(
    post_evolution_capability: i64,
    regression_cost: i64,
    review_cost: i64,
    runtime_cost: i64,
) -> NetEvolutionGain {
    NetEvolutionGain {
        post_evolution_capability,
        regression_cost,
        review_cost,
        runtime_cost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_gain_is_capability_minus_costs() {
        let g = compute_net_gain(10, 2, 1, 3);
        assert_eq!(g.net_gain(), 4);
        assert!(g.is_net_positive());
        assert_eq!(g.verdict(), "improved");
    }

    #[test]
    fn regression_detected_when_net_gain_negative() {
        // 能力 +2，但回归 +3（引入更多破坏）→ 越改越差。
        let g = compute_net_gain(2, 3, 1, 1);
        assert_eq!(g.net_gain(), -3);
        assert!(!g.is_net_positive());
        assert_eq!(g.verdict(), "regressed");
    }

    #[test]
    fn neutral_when_costs_equal_capability() {
        let g = compute_net_gain(5, 2, 2, 1);
        assert_eq!(g.net_gain(), 0);
        assert!(g.is_net_positive());
        assert_eq!(g.verdict(), "neutral");
    }
}
