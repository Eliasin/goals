use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    event::{Event, EventId},
    goal::{Goal, GoalEvent, GoalId},
    query::TimeOfDayConfiguration,
};

use self::goal_traversal::populate_goal_tree;

pub struct ProfileAndDateTime<'a>(pub &'a mut Profile, pub DateTime<Utc>);

pub mod goal_traversal {
    use std::collections::{HashMap, HashSet};

    use crate::goal::{Goal, GoalId, PopulatedGoal};

    pub type GoalChildIndexPath = Vec<usize>;

    pub fn traverse_populated_goal_children<'a>(
        root_goal: &'a mut PopulatedGoal,
        goal_child_index_path: &GoalChildIndexPath,
    ) -> Option<&'a mut PopulatedGoal> {
        let mut current = root_goal;

        for goal_child_index in goal_child_index_path {
            match current.children.get_mut(*goal_child_index) {
                Some(child) => current = child,
                None => return None,
            }
        }

        Some(current)
    }

    pub fn get_goal_parent_id(goals: &HashMap<GoalId, Goal>, goal_id: GoalId) -> Option<GoalId> {
        goals
            .iter()
            .find(|(_, goal)| goal.children().contains(&goal_id))
            .map(|(id, _)| *id)
    }

    /// Visit goals in a goal child tree. This function is especially useful for building
    /// a parallel intrinsically connected tree from the flat, ID based internal
    /// representations of goals in profiles.
    ///
    /// To facilitate this use case each invocation of the visitor can create an associated
    /// chunk of data of type V that is created through the visitor function invocation and
    /// is passed to the children when they are visited.
    ///
    /// An example use case could be summing the required effort to fully complete goals between
    /// the root of a goal tree and children within the tree.
    ///
    /// ```
    /// # use std::collections::HashMap;
    /// # use goals::profile::goal_traversal::visit_goal_child_tree;
    /// # use goals::goal::{Goal, GoalId};
    /// let mut goals: HashMap<GoalId, Goal> = HashMap::new();
    ///
    /// const ROOT_EFFORT_TO_COMPLETE: u32 = 1;
    /// const LEFT_CHILD_EFFORT_TO_COMPLETE: u32 = 1;
    /// const RIGHT_CHILD_EFFORT_TO_COMPLETE: u32 = 4;
    /// const RIGHT_GRANDCHILD_EFFORT_TO_COMPLETE: u32 = 5;
    ///
    /// let (root_goal_id, mut root_goal) = (GoalId(1), Goal::new("root", ROOT_EFFORT_TO_COMPLETE));
    /// let (left_child_goal_id, mut left_child_goal) = (
    ///     GoalId(2),
    ///     Goal::new("left-child", LEFT_CHILD_EFFORT_TO_COMPLETE),
    /// );
    /// let (right_child_goal_id, mut right_child_goal) = (
    ///     GoalId(3),
    ///     Goal::new("right-child", RIGHT_CHILD_EFFORT_TO_COMPLETE),
    /// );
    /// let (right_grandchild_goal_id, mut right_grandchild_goal) = (
    ///     GoalId(4),
    ///     Goal::new("right-grandchild", RIGHT_GRANDCHILD_EFFORT_TO_COMPLETE),
    /// );
    ///
    /// root_goal.refine(left_child_goal_id, 0);
    /// root_goal.refine(right_child_goal_id, 0);
    /// right_child_goal.refine(right_grandchild_goal_id, 0);
    ///
    /// goals.insert(root_goal_id, root_goal);
    /// goals.insert(left_child_goal_id, left_child_goal);
    /// goals.insert(right_child_goal_id, right_child_goal);
    /// goals.insert(right_grandchild_goal_id, right_grandchild_goal);
    ///
    /// let mut goal_effort_totals: Vec<(GoalId, u32)> = vec![];
    ///
    /// let visited_ids = visit_goal_child_tree(
    ///     &goals,
    ///     root_goal_id,
    ///     &mut |_, parent_effort_total, child_id, child_goal| -> u32 {
    ///         let child_effort_total = parent_effort_total + child_goal.effort_to_complete();
    ///         goal_effort_totals.push((child_id, child_effort_total));
    ///
    ///         child_effort_total
    ///     },
    ///     ROOT_EFFORT_TO_COMPLETE,
    /// )
    /// .expect("root goal to exist");
    ///
    /// // Visitation will skip the root goal
    /// assert!(!visited_ids.contains(&root_goal_id));
    /// assert!(visited_ids.contains(&left_child_goal_id));
    /// assert!(visited_ids.contains(&right_child_goal_id));
    /// assert!(visited_ids.contains(&right_grandchild_goal_id));
    ///
    /// assert!(goal_effort_totals.contains(&(
    ///     left_child_goal_id,
    ///     ROOT_EFFORT_TO_COMPLETE + LEFT_CHILD_EFFORT_TO_COMPLETE
    /// )));
    /// assert!(goal_effort_totals.contains(&(
    ///     right_child_goal_id,
    ///     ROOT_EFFORT_TO_COMPLETE + RIGHT_CHILD_EFFORT_TO_COMPLETE
    /// )));
    /// assert!(goal_effort_totals.contains(&(
    ///     right_grandchild_goal_id,
    ///     ROOT_EFFORT_TO_COMPLETE
    ///         + RIGHT_CHILD_EFFORT_TO_COMPLETE
    ///         + RIGHT_GRANDCHILD_EFFORT_TO_COMPLETE
    /// )));
    /// ```
    pub fn visit_goal_child_tree<V, VF>(
        goals: &HashMap<GoalId, Goal>,
        goal_id: GoalId,
        goal_visitor: &mut VF,
        root_visitor_data: V,
    ) -> Option<HashSet<GoalId>>
    where
        VF: FnMut(GoalId, &V, GoalId, &Goal) -> V,
    {
        if goals.get(&goal_id).is_some() {
            let mut visited_ids = HashSet::new();

            let mut needs_visiting: Vec<(GoalId, V)> = vec![(goal_id, root_visitor_data)];

            while let Some((current_goal_id, current_visitor_data)) = needs_visiting.pop() {
                let children = goals
                    .get(&current_goal_id)
                    .expect("current goal to be in profile")
                    .children();

                visited_ids.extend(children);

                for child_id in children {
                    let child = goals
                        .get(child_id)
                        .expect("child goal to be in the profile");

                    let child_visitor_data =
                        goal_visitor(current_goal_id, &current_visitor_data, *child_id, child);

                    needs_visiting.push((*child_id, child_visitor_data));
                }
            }

            Some(visited_ids)
        } else {
            None
        }
    }

    pub fn populated_goal_traversal_template(
        goal_id: GoalId,
        goal: &Goal,
        parent_goal_id: Option<GoalId>,
    ) -> PopulatedGoal {
        PopulatedGoal {
            id: goal_id,
            parent_goal_id,
            name: goal.name().to_string(),
            effort_to_date: goal.effort_to_date(),
            effort_to_complete: goal.effort_to_complete(),
            children: vec![],
        }
    }

    pub fn populate_goal_tree(
        goals: &HashMap<GoalId, Goal>,
        goal_id: GoalId,
    ) -> Option<(PopulatedGoal, HashSet<GoalId>)> {
        if let Some(goal) = goals.get(&goal_id) {
            let parent_goal_id = get_goal_parent_id(goals, goal_id);

            let mut root_populated_goal =
                populated_goal_traversal_template(goal_id, goal, parent_goal_id);

            let ids_visited = visit_goal_child_tree::<GoalChildIndexPath, _>(
                goals,
                goal_id,
                &mut |parent_goal_id: GoalId,
                      parent_index_path: &GoalChildIndexPath,
                      child_id: GoalId,
                      child_goal: &Goal|
                 -> GoalChildIndexPath {
                    let child_populated_goal_template = populated_goal_traversal_template(
                        child_id,
                        child_goal,
                        Some(parent_goal_id),
                    );

                    let current_goal_populated_template = traverse_populated_goal_children(
                        &mut root_populated_goal,
                        parent_index_path,
                    )
                    .expect("goal child index path to be valid");

                    let mut child_index_path = parent_index_path.clone();
                    child_index_path.push(current_goal_populated_template.children.len());

                    current_goal_populated_template
                        .children
                        .push(child_populated_goal_template);

                    child_index_path
                },
                vec![],
            )
            .expect("goal to be valid since it is checked before calling visit");

            Some((root_populated_goal, ids_visited))
        } else {
            None
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Profile {
    goal_id_count: u32,
    event_id_count: u32,
    focused_goals: HashSet<GoalId>,
    pub(crate) goals: HashMap<GoalId, Goal>,
    pub(crate) events: HashMap<EventId, Event>,
    pub(crate) time_of_day_config: TimeOfDayConfiguration,
}

impl Profile {
    pub fn time_of_day_config(&self) -> &TimeOfDayConfiguration {
        &self.time_of_day_config
    }

    pub fn set_time_of_day_config(&mut self, config: TimeOfDayConfiguration) {
        self.time_of_day_config = config;
    }

    pub fn focus_goal(&mut self, id: GoalId) -> Option<GoalEvent> {
        self.goals.get(&id).map(|_| {
            self.focused_goals.insert(id);

            GoalEvent::Focus {
                focus_root_id: id,
                focused_children: vec![],
            }
        })
    }

    pub fn add_goal(&mut self, goal: Goal) -> GoalEvent {
        let goal_id = GoalId(self.goal_id_count);
        self.goal_id_count += 1;

        if self.goals.insert(goal_id, goal).is_some() {
            panic!("not to have a goal id conflict due to monotonic counter");
        }

        GoalEvent::Add { goal_id }
    }

    pub fn refine_goal(
        &mut self,
        child_goal: Goal,
        parent_goal_id: GoalId,
        parent_effort_removed: u32,
    ) -> Option<GoalEvent> {
        let Some(parent_goal) = self.goals.get_mut(&parent_goal_id) else {
                return None;
            };

        let child_goal_id = GoalId(self.goal_id_count);
        self.goal_id_count += 1;

        parent_goal.refine(child_goal_id, parent_effort_removed);

        if self.goals.insert(child_goal_id, child_goal).is_some() {
            panic!("not to have a goal id conflict due to monotonic counter");
        }

        Some(GoalEvent::Refine {
            parent_goal_id,
            parent_effort_removed,
            new_child_goal: child_goal_id,
        })
    }

    fn remove_goals_from_event_relationships(&mut self, goal_ids: &HashSet<GoalId>) {
        for event in &mut self.events.values_mut() {
            event.goal_relationships_mut().retain(|goal| match goal {
                crate::goal::GoalRelationship::Requires(id) => goal_ids.contains(id),
                crate::goal::GoalRelationship::Ends(id) => goal_ids.contains(id),
                crate::goal::GoalRelationship::WorksOn(id) => goal_ids.contains(id),
                crate::goal::GoalRelationship::Starts(id) => goal_ids.contains(id),
            })
        }
    }

    pub fn remove_goal(&mut self, goal_id: GoalId) -> Option<GoalEvent> {
        if let Some((populated_goal, ids_needing_removal)) =
            populate_goal_tree(&self.goals, goal_id)
        {
            for goal_id in ids_needing_removal.iter() {
                self.goals.remove(goal_id);
                self.focused_goals.remove(goal_id);
            }

            self.remove_goals_from_event_relationships(&ids_needing_removal);

            Some(GoalEvent::Delete {
                deleted_goal_data: populated_goal,
            })
        } else {
            None
        }
    }

    pub fn add_event(&mut self, event: Event) -> EventId {
        let event_id = EventId(self.event_id_count);
        self.event_id_count += 1;

        if self.events.insert(event_id, event).is_some() {
            panic!("not to have an event id conflict due to monotonic counter");
        }

        event_id
    }

    pub fn remove_event(&mut self, event_id: EventId) -> Option<Event> {
        self.events.remove(&event_id)
    }

    pub fn with_datetime(&mut self, datetime: DateTime<Utc>) -> ProfileAndDateTime {
        ProfileAndDateTime(self, datetime)
    }

    pub fn get_event(&self, id: EventId) -> Option<&Event> {
        self.events.get(&id)
    }

    pub fn get_event_mut(&mut self, id: EventId) -> Option<&mut Event> {
        self.events.get_mut(&id)
    }

    pub fn get_goal(&self, id: GoalId) -> Option<&Goal> {
        self.goals.get(&id)
    }

    pub fn get_goal_mut(&mut self, id: GoalId) -> Option<&mut Goal> {
        self.goals.get_mut(&id)
    }
}

impl<'a> ProfileAndDateTime<'a> {
    pub fn get_event(&self, id: EventId) -> Option<&Event> {
        self.0.events.get(&id)
    }

    pub fn get_event_mut(&mut self, id: EventId) -> Option<&mut Event> {
        self.0.events.get_mut(&id)
    }

    pub fn get_goal(&self, id: GoalId) -> Option<&Goal> {
        self.0.goals.get(&id)
    }

    pub fn get_goal_mut(&mut self, id: GoalId) -> Option<&mut Goal> {
        self.0.goals.get_mut(&id)
    }
}

#[cfg(test)]
mod tests {
    mod goal_query {
        use std::collections::HashSet;

        use chrono::{TimeZone, Utc};

        use crate::{
            goal::{Goal, GoalEvent},
            profile::Profile,
            query::GoalQueryEngine,
        };

        #[test]
        fn goal_finish_status() {
            let mut profile = Profile::default();

            let datetime = Utc.with_ymd_and_hms(2022, 1, 1, 1, 0, 0).unwrap();
            let mut profile = profile.with_datetime(datetime);

            let GoalEvent::Add { goal_id } = profile.0.add_goal(Goal::new("test goal", 2)) else {
                    panic!("unexpected goal event contents");
                };

            assert!(!profile.get_goal(goal_id).unwrap().finished());
            assert_eq!(profile.unfinished_goals(), HashSet::from([goal_id]));
            assert_eq!(profile.finished_goals(), HashSet::from([]));

            profile.get_goal_mut(goal_id).unwrap().add_effort(1);
            assert!(!profile.get_goal(goal_id).unwrap().finished());
            assert_eq!(profile.unfinished_goals(), HashSet::from([goal_id]));
            assert_eq!(profile.finished_goals(), HashSet::from([]));

            profile.get_goal_mut(goal_id).unwrap().add_effort(1);
            assert!(profile.get_goal(goal_id).unwrap().finished());
            assert_eq!(profile.unfinished_goals(), HashSet::from([]));
            assert_eq!(profile.finished_goals(), HashSet::from([goal_id]));

            profile.get_goal_mut(goal_id).unwrap().rescope(4);
            assert!(!profile.get_goal(goal_id).unwrap().finished());
            assert_eq!(profile.unfinished_goals(), HashSet::from([goal_id]));
            assert_eq!(profile.finished_goals(), HashSet::from([]));

            profile.get_goal_mut(goal_id).unwrap().add_effort(1);
            assert!(!profile.get_goal(goal_id).unwrap().finished());
            assert_eq!(profile.unfinished_goals(), HashSet::from([goal_id]));
            assert_eq!(profile.finished_goals(), HashSet::from([]));

            profile.get_goal_mut(goal_id).unwrap().add_effort(1);
            assert!(profile.get_goal(goal_id).unwrap().finished());
            assert_eq!(profile.unfinished_goals(), HashSet::from([]));
            assert_eq!(profile.finished_goals(), HashSet::from([goal_id]));
        }

        #[test]
        fn goal_deletion() {
            let mut profile = Profile::default();

            let datetime = Utc.with_ymd_and_hms(2022, 1, 1, 1, 0, 0).unwrap();

            let profile = profile.with_datetime(datetime);
            let GoalEvent::Add { goal_id } = profile.0.add_goal(Goal::new("test goal", 10)) else {
                    panic!("unexpected goal event contents");
                };

            profile.0.refine_goal(Goal::new("child", 2), goal_id, 0);
        }
    }
}
