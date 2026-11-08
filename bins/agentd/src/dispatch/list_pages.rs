use super::*;
use app_services::Clock;
use ipc_contracts::proto::{ListProjectsRequest, ListTasksForProjectRequest};
use prost::Message;

const BYTE_BUDGET: usize = ipc_transport::MAX_FRAME_BYTES - 1024;

fn valid_page(after: &str, size: u32) -> bool {
    if size == 0 {
        return after.is_empty();
    }
    size > 0
        && size <= ipc_contracts::MAX_LIST_PAGE_ITEMS
        && (after.is_empty() || TaskId::new(after).is_ok_and(|id| id.as_str() == after))
}

pub(super) fn projects<C: Clock>(
    service: &TaskService<C>,
    request: ListProjectsRequest,
) -> TaskCoreResponse {
    if !valid_page(&request.after_id, request.page_size) {
        return invalid_argument("invalid list page size or cursor");
    }
    let legacy = request.page_size == 0;
    let fetch_limit = if legacy {
        u32::MAX
    } else {
        request.page_size + 1
    };
    let available = match service.list_projects_page(&request.after_id, fetch_limit, BYTE_BUDGET) {
        Ok(items) => items,
        Err(error) => return error_response(error),
    };
    let mut reply = ListProjectsResponse::default();
    let mut bytes = 0;
    let mut more = false;
    for project in available {
        let dto = project_dto(&project);
        let cost = dto.encoded_len() + 10;
        if (!legacy && reply.projects.len() == request.page_size as usize)
            || bytes + cost > BYTE_BUDGET
        {
            more = true;
            break;
        }
        bytes += cost;
        reply.projects.push(dto);
    }
    if more {
        if legacy {
            return invalid_argument("list exceeds one frame; use task-list-pages-v1");
        }
        let Some(last) = reply.projects.last() else {
            return invalid_argument("project exceeds the list frame budget");
        };
        reply.next_after_id.clone_from(&last.id);
    }
    success(task_core_response::Payload::ListProjects(reply))
}

pub(super) fn tasks<C: Clock>(
    service: &TaskService<C>,
    request: ListTasksForProjectRequest,
) -> TaskCoreResponse {
    if !valid_page(&request.after_id, request.page_size) {
        return invalid_argument("invalid list page size or cursor");
    }
    let legacy = request.page_size == 0;
    let fetch_limit = if legacy {
        u32::MAX
    } else {
        request.page_size + 1
    };
    let project = match ProjectId::new(request.project_id) {
        Ok(id) => id,
        Err(error) => return error_response(ServiceError::Domain(error)),
    };
    let available = match service.list_task_summaries_page(
        &project,
        &request.after_id,
        fetch_limit,
        BYTE_BUDGET,
    ) {
        Ok(items) => items,
        Err(error) => return error_response(error),
    };
    let mut reply = ListTasksResponse::default();
    let mut bytes = 0;
    let mut more = false;
    for task in available {
        let dto = TaskDto {
            id: task.id.to_string(),
            project_id: task.project_id.to_string(),
            objective: task.objective,
            state: task.state.as_str().to_owned(),
            depends_on: task.depends_on.iter().map(ToString::to_string).collect(),
        };
        let cost = dto.encoded_len() + 10;
        if (!legacy && reply.tasks.len() == request.page_size as usize)
            || bytes + cost > BYTE_BUDGET
        {
            more = true;
            break;
        }
        bytes += cost;
        reply.tasks.push(dto);
    }
    if more {
        if legacy {
            return invalid_argument("list exceeds one frame; use task-list-pages-v1");
        }
        let Some(last) = reply.tasks.last() else {
            return invalid_argument("task exceeds the list frame budget");
        };
        reply.next_after_id.clone_from(&last.id);
    }
    success(task_core_response::Payload::ListTasks(reply))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(dir: &std::path::Path) -> TaskService<app_services::FixedClock> {
        TaskService::attach(
            storage_sqlite::SqliteStore::open(dir.join("state.sqlite3")).unwrap(),
            app_services::FixedClock(1000),
        )
    }

    #[test]
    fn large_objectives_are_retrieved_through_bounded_pages_and_legacy_fails_explicitly() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = service(dir.path());
        let project = ProjectId::new("project").unwrap();
        service
            .create_project(&Project::new(project.clone(), "project").unwrap())
            .unwrap();
        for id in ["a", "b"] {
            service
                .create_task(
                    &Task::draft(
                        TaskId::new(id).unwrap(),
                        project.clone(),
                        "x".repeat(600_000),
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        service.mark_ready(&TaskId::new("a").unwrap()).unwrap();
        let other = ProjectId::new("other").unwrap();
        service
            .create_project(&Project::new(other.clone(), "other").unwrap())
            .unwrap();
        service
            .create_task(
                &Task::draft(TaskId::new("aa").unwrap(), other, "must not appear").unwrap(),
            )
            .unwrap();
        let legacy = dispatch(
            &mut service,
            TaskCoreRequest {
                payload: Some(task_core_request::Payload::ListTasksForProject(
                    ListTasksForProjectRequest {
                        project_id: project.to_string(),
                        ..Default::default()
                    },
                )),
            },
            None,
        );
        assert!(matches!(
            legacy.payload,
            Some(task_core_response::Payload::Error(_))
        ));
        let mut after_id = String::new();
        let mut ids = Vec::new();
        for page in 0..2 {
            let response = tasks(
                &service,
                ListTasksForProjectRequest {
                    project_id: project.to_string(),
                    after_id: after_id.clone(),
                    page_size: 256,
                },
            );
            let envelope = ipc_contracts::proto::ControlResponse {
                payload: Some(ipc_contracts::proto::control_response::Payload::TaskCore(
                    response.clone(),
                )),
            };
            assert!(envelope.encoded_len() <= ipc_transport::MAX_FRAME_BYTES);
            let Some(task_core_response::Payload::ListTasks(reply)) = response.payload else {
                panic!("page failed");
            };
            assert_eq!(reply.tasks.len(), 1);
            assert_eq!(reply.tasks[0].objective, "x".repeat(600_000));
            assert_eq!(reply.tasks[0].project_id, project.as_str());
            assert_eq!(
                reply.tasks[0].state,
                if page == 0 { "READY" } else { "DRAFT" }
            );
            ids.push(reply.tasks[0].id.clone());
            assert_eq!(reply.next_after_id.is_empty(), page == 1);
            after_id = reply.next_after_id;
        }
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn task_pages_carry_dependency_ids_for_their_own_tasks_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = service(dir.path());
        let project = ProjectId::new("project").unwrap();
        service
            .create_project(&Project::new(project.clone(), "project").unwrap())
            .unwrap();
        for id in ["a", "b", "c"] {
            service
                .create_task(&Task::draft(TaskId::new(id).unwrap(), project.clone(), "x").unwrap())
                .unwrap();
        }
        for (task, target) in [("c", "b"), ("c", "a"), ("b", "a")] {
            service
                .add_dependency(&TaskId::new(task).unwrap(), &TaskId::new(target).unwrap())
                .unwrap();
        }
        let mut seen = Vec::new();
        let mut after_id = String::new();
        loop {
            let response = tasks(
                &service,
                ListTasksForProjectRequest {
                    project_id: project.to_string(),
                    after_id: after_id.clone(),
                    page_size: 2,
                },
            );
            let Some(task_core_response::Payload::ListTasks(reply)) = response.payload else {
                panic!("page failed");
            };
            seen.extend(reply.tasks.into_iter().map(|t| (t.id, t.depends_on)));
            if reply.next_after_id.is_empty() {
                break;
            }
            after_id = reply.next_after_id;
        }
        assert_eq!(
            seen,
            [
                ("a".to_owned(), vec![]),
                ("b".to_owned(), vec!["a".to_owned()]),
                ("c".to_owned(), vec!["a".to_owned(), "b".to_owned()]),
            ]
        );
    }

    #[test]
    fn count_pages_preserve_order_and_reject_invalid_cursors() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = service(dir.path());
        for id in ["a", "b", "c"] {
            service
                .create_project(&Project::new(ProjectId::new(id).unwrap(), id).unwrap())
                .unwrap();
        }
        let first = projects(
            &service,
            ListProjectsRequest {
                after_id: String::new(),
                page_size: 2,
            },
        );
        let Some(task_core_response::Payload::ListProjects(first)) = first.payload else {
            panic!("page failed");
        };
        assert_eq!(
            first
                .projects
                .iter()
                .map(|project| project.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(first.next_after_id, "b");
        let next = projects(
            &service,
            ListProjectsRequest {
                after_id: first.next_after_id,
                page_size: 2,
            },
        );
        let Some(task_core_response::Payload::ListProjects(next)) = next.payload else {
            panic!("page failed");
        };
        assert_eq!(next.projects[0].id, "c");
        assert!(next.next_after_id.is_empty());
        let legacy = projects(&service, ListProjectsRequest::default());
        let Some(task_core_response::Payload::ListProjects(legacy)) = legacy.payload else {
            panic!("legacy list failed");
        };
        assert_eq!(legacy.projects.len(), 3);
        assert!(legacy.next_after_id.is_empty());
        for (after_id, page_size) in [(" bad ", 1), ("", 257), ("a", 0)] {
            let reply = projects(
                &service,
                ListProjectsRequest {
                    after_id: after_id.to_owned(),
                    page_size,
                },
            );
            assert!(matches!(
                reply.payload,
                Some(task_core_response::Payload::Error(_))
            ));
        }
    }
}
