use ecs::{Entities, Executor, World};

fn print_system(entities: Entities<&i32>, res: &i32) {
    for int in entities {
        println!("print sys {int} ({res})");
    }
}

fn bool_system(entities: Entities<&bool>) {
    for b in entities {
        assert!(b);
    }
}

fn increment_system(entities: Entities<&mut i32>, res: &mut i32) {
    *res += 1;
    for i in entities {
        *i += 1;
    }
}

fn assert_system(entities: Entities<(&i32, &u8)>, res: &i32) {
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
    let mut world = World::new();
    let mut executor = Executor::new();

    executor.add_resource(0);
    assert_eq!(0, *executor.get_resource::<i32>().unwrap());

    world.spawn((12, true, 0u8));
    world.spawn((420, true, 1u8));
    world.spawn((56, 2u8));

    let schedule = executor
        .schedule()
        .then(bool_system)
        .then(increment_system)
        .then(print_system)
        .build();

    let assert = executor.schedule().then(assert_system).build();

    for _ in 0..4 {
        executor.execute(&schedule, &mut world);
    }

    executor.execute(&assert, &mut world);
}
