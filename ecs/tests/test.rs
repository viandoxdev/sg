use std::{collections::HashSet, sync::Arc};

use ecs::{Entities, Entity, Executor, World};

fn print_system(entities: Entities<&i32>, res: &i32) {
    log::debug!("PRINT SYSTEM");
    for int in entities {
        println!("print sys {int} ({res})");
    }
}

fn bool_system(entities: Entities<&bool>) {
    log::debug!("BOOL SYSTEM");
    for b in entities {
        assert!(b);
    }
}

fn increment_system(entities: Entities<&mut i32>, res: &mut i32) {
    log::debug!("INCREMENT SYSTEM");
    *res += 1;
    for i in entities {
        *i += 1;
    }
}

fn assert_system(entities: Entities<(&i32, &u8)>, res: &i32) {
    log::debug!("ASSERT SYSTEM");
    assert_eq!(*res, 4);
    let mut entities = entities.map(|(i, u)| (*i, *u)).collect::<Vec<_>>();
    entities.sort_by_key(|(_, u)| *u);
    assert_eq!((16, 0u8), entities[0]);
    assert_eq!((424, 1u8), entities[1]);
    assert_eq!((60, 2u8), entities[2]);
    assert_eq!(None, entities.get(3));
}

#[test]
fn basic() {
    env_logger::init();

    let mut world = World::new();
    let mut executor = Executor::new();

    executor.add_resource(0);
    assert_eq!(0, *executor.get_resource::<i32>().unwrap());

    let entities = [
        world.spawn((12, true, 0u8)),
        world.spawn((420, true, 1u8)),
        world.spawn((56, 2u8)),
    ]
    .into_iter()
    .collect::<HashSet<_>>();

    let schedule = executor
        .schedule()
        .then(bool_system)
        .then(increment_system)
        .then(print_system)
        .build();

    for _ in 0..4 {
        executor.execute(&schedule, &mut world);
    }

    let assert = executor.schedule_single(assert_system);
    executor.execute(&assert, &mut world);

    executor.execute_single(
        move |e: Entities<Entity>| {
            let es = e.collect::<Vec<_>>();
            assert_eq!(es.len(), entities.len());
            for entity in &es {
                println!("Entity {entity:?}");
                assert!(entities.contains(entity));
            }
        },
        &mut world,
    );
}
