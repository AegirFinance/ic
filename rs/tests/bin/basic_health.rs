use anyhow::Result;
use ic_tests::basic_health_test;
use ic_tests::driver::new::dsl::SystemTestGroup;
use ic_tests::systest;

fn main() -> Result<()> {
    SystemTestGroup::new()
        .with_setup(basic_health_test::config_single_host)
        .add_test(systest!(basic_health_test::test))
        .execute_from_args()?;

    Ok(())
}
