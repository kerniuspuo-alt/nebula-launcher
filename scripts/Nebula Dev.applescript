on run
	set supportPath to "/Users/kerniuspuodzius/Library/Application Support/Nebula"
	set logPath to "/Users/kerniuspuodzius/Library/Logs/Nebula Dev.log"
	set lockPath to supportPath & "/dev.pid"
	set savedPath to supportPath & "/project-path.txt"
	do shell script "/bin/mkdir -p " & quoted form of supportPath & " " & quoted form of "/Users/kerniuspuodzius/Library/Logs"
	set projectPath to ""
	try
		set projectPath to do shell script "/bin/cat " & quoted form of savedPath
	end try
	if projectPath is "" then
		set projectFolder to choose folder with prompt "Choose the Nebula Launcher source folder for live updates"
		set projectPath to POSIX path of projectFolder
		do shell script "/usr/bin/printf %s " & quoted form of projectPath & " > " & quoted form of savedPath
	end if
	set commandText to "/bin/mkdir -p " & quoted form of supportPath & " " & quoted form of "/Users/kerniuspuodzius/Library/Logs" & "; " & ¬
		"if [ -f '" & lockPath & "' ] && /bin/kill -0 \"$(/bin/cat '" & lockPath & "')\" 2>/dev/null; then exit 0; fi; " & ¬
		"cd " & quoted form of projectPath & " && { PATH=/usr/local/bin:$HOME/.cargo/bin:/usr/bin:/bin /usr/bin/nohup /usr/local/bin/npm run app:dev >>" & quoted form of logPath & " 2>&1 </dev/null & echo $! >" & quoted form of lockPath & "; }"
	do shell script commandText
end run