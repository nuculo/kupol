with open('src/server/mod.rs', 'r') as f:
    server_lines = f.readlines()

# The actual server code ends at line 150 (index 149)
# specifically at: "    }))\n}\n\n"
# Let's cleanly separate it.
server_end_idx = -1
for i, line in enumerate(server_lines):
    if line.strip() == "// B. ETS-подобная разделяемая память без блокировок":
        server_end_idx = i
        break

if server_end_idx != -1:
    pure_server = server_lines[:server_end_idx]
    demos_tail = server_lines[server_end_idx:]

    with open('src/server/mod.rs', 'w') as f:
        f.writelines(pure_server)

    with open('src/demos/mod.rs', 'a') as f:
        f.writelines(demos_tail)
    
    print(f"Moved {len(demos_tail)} lines from server to demos.")
else:
    print("Could not find split point in server/mod.rs")
