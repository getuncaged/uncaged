use super::minimal_tab_title;

#[test]
fn shell_titles_become_the_folder_name() {
    assert_eq!(minimal_tab_title("anton@Antons-MacBook-Pro:~"), "~");
    assert_eq!(
        minimal_tab_title("anton@Antons-MacBook-Pro:~/Brain"),
        "Brain"
    );
    assert_eq!(minimal_tab_title("anton@host: ~/code/uncaged/"), "uncaged");
    assert_eq!(minimal_tab_title("root@box:/usr/local/bin"), "bin");
    assert_eq!(minimal_tab_title("root@box:/"), "/");
}

#[test]
fn shell_truncated_titles_become_the_folder_name() {
    // oh-my-zsh cuts the title from the left before Uncaged ever sees it.
    assert_eq!(minimal_tab_title("..v@Antons-MacBook-Pro:~"), "~");
    assert_eq!(minimal_tab_title("..ns-MacBook-Pro:~/Brain"), "Brain");
    assert_eq!(minimal_tab_title("Antons-MacBook-Pro:/tmp"), "tmp");
    // Deep enough that the host is cut off as well.
    assert_eq!(
        minimal_tab_title("..lexiMeal/flexi-meal-ai"),
        "flexi-meal-ai"
    );
    // A program title that merely starts with dots is left alone.
    assert_eq!(minimal_tab_title("...loading"), "...loading");
}

#[test]
fn bare_paths_become_the_folder_name() {
    assert_eq!(minimal_tab_title("~/Developer/Uncaged"), "Uncaged");
    assert_eq!(minimal_tab_title("~"), "~");
}

#[test]
fn program_titles_are_kept() {
    assert_eq!(minimal_tab_title("vim notes.md"), "vim notes.md");
    assert_eq!(minimal_tab_title("htop"), "htop");
    assert_eq!(
        minimal_tab_title("npm run dev: watching"),
        "npm run dev: watching"
    );
    assert_eq!(minimal_tab_title("  zsh  "), "zsh");
}
