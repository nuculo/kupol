# If split.py or split2.py failed, the content might be mangled.
# I need to find the full run_demos() logic from somewhere. 
# Luckily, demos.rs from split.py has the full extracted run_demos().
# I will stitch them back and then do a proper python parsing.

import os

try:
    with open('src/main.rs', 'r') as f:
        main_content = f.read()
    with open('src/demos.rs', 'r') as f:
        demos_content = f.read()
    
    # demos_content has the header, we strip it out.
    idx = demos_content.find("pub async fn run_demos()")
    if idx != -1:
        pure_demos = demos_content[idx:].replace("pub async fn run_demos()", "async fn run_demos()")
        
        # Combine back
        # main_content might have the 'crate::demos::run_demos().await?;' and 'pub mod demos;'
        main_content = main_content.replace('crate::demos::run_demos().await?;', 'run_demos().await?;')
        main_content = main_content.replace('pub mod kan_intelligence;\n/// Демонстрационные сценарии\npub mod demos;', 'pub mod kan_intelligence;')

        # Fix where main_content was cut
        # split2.py cut at "// ─────────────────────────────────────────────────────────────────────────────\n// § Демо-сценарии (вынесены из main)"
        # so we append it back.
        
        if "// § Демо-сценарии" not in main_content:
            main_content += "\n// ─────────────────────────────────────────────────────────────────────────────\n// § Демо-сценарии (вынесены из main)\n// ─────────────────────────────────────────────────────────────────────────────\n\n"
            main_content += pure_demos

            with open('src/main.rs', 'w') as f:
                f.write(main_content)
            
            print("main.rs restored successfully.")
        else:
            print("main.rs looks like it already has demos. I will not overwrite.")
except Exception as e:
    print(f"Error restoring: {e}")
