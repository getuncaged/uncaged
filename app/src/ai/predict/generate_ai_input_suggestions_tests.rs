use crate::terminal::model::terminal_model::BlockIndex;
use crate::terminal::model::test_utils::create_test_block_with_grids;
use crate::test_util::mock_blockgrid;

#[test]
fn test_get_block_content_summary_extreme_narrow_vs_wide() {
    // Create blocks with many lines to see the difference in truncation
    let mut massive_input = String::new();
    for i in 0..1000 {
        massive_input.push_str(&format!("Input line {i} with some content\r\n"));
    }
    let prompt_grid = mock_blockgrid(&massive_input);
    let rprompt_grid = mock_blockgrid("");

    let mut massive_output = String::new();
    for i in 0..1000 {
        massive_output.push_str(&format!("Output line {i} with some content\r\n"));
    }
    let output_grid = mock_blockgrid(&massive_output);

    // Note terminal model isn't being used here (i.e. we're not creating a test model
    // and using the # of columns from it), instead we simulate this test.
    let block = create_test_block_with_grids(
        BlockIndex::zero(),
        prompt_grid,
        rprompt_grid,
        output_grid,
        false, // honor_ps1
    );

    // Test with extremely narrow terminal
    let (input_narrow, output_narrow) = block.get_block_content_summary(
        20, // Very narrow terminal
        75, // Top lines
        25, // Bottom lines
    );

    // Test with wide terminal
    let (input_wide, output_wide) = block.get_block_content_summary(
        200, // Wide terminal
        75,  // Top lines
        25,  // Bottom lines
    );

    // Narrow terminal - top should have 562 lines and bottom should have 188 lines.

    assert!(input_narrow.contains("Input line 561"));
    assert!(!input_narrow.contains("Input line 562"));
    assert!(!input_narrow.contains("Input line 812"));
    assert!(input_narrow.contains("Input line 813"));

    assert!(output_narrow.contains("Output line 561"));
    assert!(!output_narrow.contains("Output line 562"));
    assert!(!output_narrow.contains("Output line 812"));
    assert!(output_narrow.contains("Output line 813"));

    // Wide terminal - top should have 75 lines and bottom should have 25 lines.
    assert!(input_wide.contains("Input line 74"));
    assert!(!input_wide.contains("Input line 75"));
    assert!(!input_wide.contains("Input line 974"));
    assert!(input_wide.contains("Input line 975"));

    assert!(output_wide.contains("Output line 74"));
    assert!(!output_wide.contains("Output line 75"));
    assert!(!output_wide.contains("Output line 974"));
    assert!(output_wide.contains("Output line 975"));
}
