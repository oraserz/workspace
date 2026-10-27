-- Installation shell only. Native C owns every game frame, control and save.
local lvgl=require('lvgl')
local EXPECT_FW='3.101.043'
local EXPECT_BUILD=825192154
local EXPECT_BYTES=726540
local EXPECT_FW_CRC=3944353756
local INSTALLER_VERSION='0.7.3'
local MODULE='minecraft10_073'
local STATUS='/dev/minecraft10_073'
local AP_BYTES=13795728
local DEST='/data/minecraft10-073.elf'
local ATTEMPT='/data/minecraft10-073-attempt.txt'
local base=SCRIPT_PATH or ''
local function resource(n)return base..((base=='' or base:sub(-1)=='/')and''or'/')..n end
local root=lvgl.Object(nil,{w=336,h=480,x=0,y=0,bg_color=0x18212a,bg_opa=255,border_width=0,pad_all=0})
root:clear_flag(lvgl.FLAG.SCROLLABLE)
local props={x=18,y=52,w=300,text='Minecraft · 10 Pro\n原生安装 0.7.3\n正在核对固件…',text_color=0xffffff,pad_all=0}
local ok,font=pcall(lvgl.Font,'MiSans-Regular',20);if ok then props.text_font=font end
local label=lvgl.Label(root,props)
local timer,source,target,verify,phase,at,total,polls
local destroyed,finished=false,false
local retry=false
local clockActive=false
local lastStage,interruptedStage,nativeStage="",nil,nil
local function attempt(text)
 local f=assert(io.open(ATTEMPT,'wb'),'E_ATTEMPT_OPEN')
 local worked,err=pcall(function()assert(f:write(text),'E_ATTEMPT_WRITE');assert(f:flush(),'E_ATTEMPT_FLUSH')end)
 local closed,result=pcall(f.close,f);assert(worked,err);assert(closed and result~=nil and result~=false,'E_ATTEMPT_CLOSE')
end
local function stage(text)
 if text~=lastStage then attempt(text);lastStage=text end
end
local function previousAttempt()
 local f=io.open(ATTEMPT,'rb');if not f then return '' end
 local b=f:read(100)or'';local ok=f:close();assert(ok,'E_ATTEMPT_CLOSE');return b
end
local function close(f)if f then local a=f:close();assert(a~=nil and a~=false,'E_CLOSE')end end
local function closeAll()
 if source then pcall(source.close,source)end
 if target then pcall(target.close,target)end
 if verify then pcall(verify.close,verify)end
 source,target,verify=nil,nil,nil
end
local function finish(text)
 finished=true;closeAll();if timer then pcall(timer.delete,timer);timer=nil end
 if not destroyed then label:set{text=text}end
end
local function fail(err)
 local code=tostring(err):match('(E_[A-Z0-9_:%-]+)')or'E_UNKNOWN'
 local trace=interruptedStage or nativeStage or lastStage:match('^([A-Z][A-Z0-9]*)') or '-'
 if code=='E_OLD_MODULE' then
  retry=false;finish('安装未完成 · 0.7.3\nE_OLD_MODULE\n阶段 OLD\n\n请先重启手环\n再选择此安装表盘');return
 end
 retry=true;finish('安装未完成 · 0.7.3\n'..code:sub(1,28)..'\n阶段 '..trace:sub(1,12)..'\n固件 '..EXPECT_FW..'\n\n拍这一屏即可\n停止自动重试\n点击此页重试')
end
local function read(path,limit)
 local f=assert(io.open(path,'rb'),'E_READ_OPEN');local b=f:read(limit+1)or'';close(f);assert(#b<=limit,'E_READ_SIZE');return b
end
local function execute(cmd,path)
 local f=assert(io.open(path,'wb'),'E_CAPTURE');assert(f:write(''),'E_CAPTURE');close(f)
 local a,b,c=os.execute(cmd..' > '..path)
 local code
 if b=='exit' and type(c)=='number' and c%1==0 then code=c>=0 and c<=255 and c or math.floor((c%4294967296)/256)%256
 elseif type(a)=='number' and a%1==0 and a%256==0 then code=math.floor((a%4294967296)/256)%256
 elseif a==true and b==nil then code=0 end
 assert(code~=nil,'E_SHELL_STATUS');assert(code==0,'E_SHELL:'..tostring(code))
 return read(path,8192)
end
local function u32(b,at)local a,c,d,e=b:byte(at+1,at+4);assert(e,'E_STATUS_SIZE');return a+c*256+d*65536+e*16777216 end
local function xor(a,b)local n,p=0,1;for _=1,32 do if a%2~=b%2 then n=n+p end;a=math.floor(a/2);b=math.floor(b/2);p=p*2 end;return n end
local function crc(b)local c=4294967295;for i=1,#b do c=xor(c,b:byte(i));for _=1,8 do local bit=c%2;c=math.floor(c/2);if bit==1 then c=xor(c,3988292384)end end end;return 4294967295-c end
local function state()
 local f=assert(io.open(STATUS,'rb'),'E_DEVICE_OPEN')
 local b=f:read(40)or'';close(f)
 assert(#b==40 and u32(b,0)==0x4730314e and crc(b:sub(1,36))==u32(b,36),'E_STATUS_CRC')
 assert(u32(b,4)==EXPECT_BUILD and u32(b,24)==EXPECT_FW_CRC,'E_OLD_MODULE')
 local currentPhase=u32(b,12);nativeStage=(currentPhase>=20 and currentPhase<=25) and ('R'..(currentPhase-20)) or nil
 local error=u32(b,16);if error>=2147483648 then error=error-4294967296 end
 assert(error==0,'E_NATIVE:'..u32(b,12)..':'..error)
 return {ready=u32(b,8)==1,phase=u32(b,12),hashed=u32(b,28),sequence=u32(b,32)}
end
local function le(n)return string.char(n%256,math.floor(n/256)%256,math.floor(n/65536)%256,math.floor(n/16777216)%256)end
local function step(sequence)
 -- file write is synchronous on THIS Lua UI timer; never shell-redirection.
 local f=assert(io.open(STATUS,'wb'),'E_DEVICE_OPEN')
 local worked,err=pcall(function()
  assert(f:write(le(0x324c434d)..le(EXPECT_BUILD)..le(sequence)),'E_DEVICE_WRITE')
  assert(f:flush(),'E_DEVICE_FLUSH')
 end)
 local closed,result=pcall(f.close,f)
 assert(worked,err);assert(closed and result~=nil and result~=false,'E_DEVICE_CLOSE')
end
-- System dataman owns the subscription and removes it when this label deletes.
-- Stop the installer poll before showing time; no native registration in here.
local function showClock()
 finished=true;retry=false;clockActive=true;closeAll()
 if timer then pcall(timer.delete,timer);timer=nil end
 if destroyed then return end
 root:set{bg_color=0x000000}
 local clockProps={x=12,y=184,w=312,h=112,text='--:--',text_align=1,text_color=0xffffff,pad_all=0}
 local ok,clockFont=pcall(lvgl.Font,'MiSans-Regular',72)
 if ok then clockProps.text_font=clockFont end
 label:set(clockProps)
 local previousTime
 local function refresh(obj)
  if destroyed or not clockActive then return end
  local ok,text=pcall(function()return os.date('%H:%M',os.time())end)
  if ok and type(text)=='string' and text~=previousTime then
   previousTime=text;obj:set{text=text}
  end
 end
 refresh(label)
 local ok,dataman=pcall(require,'dataman')
 local subscribed=false
 if ok and type(dataman)=='table' and type(dataman.subscribe)=='function' then
  local subscribeOk,result=pcall(dataman.subscribe,'timeMinuteLow',label,refresh)
  subscribed=subscribeOk and result~=false
 end
 if not subscribed then
  -- Compatible fallback; never restore the fast installer polling loop.
  timer=lvgl.Timer{period=60000,repeat_count=-1,cb=function()refresh(label)end}
 end
end
local function ready()
 attempt('READY '..EXPECT_FW..' '..EXPECT_BUILD);retry=false
 showClock()
end
local function beginCopy(name,destination,bytes)
 source=assert(io.open(resource(name),'rb'),'E_RESOURCE')
 local size=assert(source:seek('end',0),'E_RESOURCE_SEEK');assert(size==bytes,'E_RESOURCE_SIZE');assert(source:seek('set',0),'E_RESOURCE_SEEK')
 target=assert(io.open(destination,'wb'),'E_COPY_OPEN');at,total=0,size;phase='COPY'
end
local destination=DEST
local function tick()
 if destroyed or finished then return end
 local worked,err=pcall(function()
  if phase=='BEGIN' then
   local fw=execute('getprop ro.build.version','/tmp/minecraft10-version.txt'):match('^%s*([%d._]+)')
   assert(fw and fw:gsub('_','.')==EXPECT_FW,'E_FIRMWARE')
   local modules=execute('lsmod','/tmp/minecraft10-modules.txt')
   assert(not modules:find('%f[%w_]minecraft10_041%f[^%w_]') and not modules:find('%f[%w_]minecraft10_042%f[^%w_]') and not modules:find('%f[%w_]minecraft10_060%f[^%w_]') and not modules:find('%f[%w_]minecraft10_061%f[^%w_]') and not modules:find('%f[%w_]minecraft10_062%f[^%w_]') and not modules:find('%f[%w_]minecraft10_063%f[^%w_]') and not modules:find('%f[%w_]minecraft10_064%f[^%w_]') and not modules:find('%f[%w_]minecraft10_065%f[^%w_]') and not modules:find('%f[%w_]minecraft10_066%f[^%w_]') and not modules:find('%f[%w_]minecraft10_067%f[^%w_]') and not modules:find('%f[%w_]minecraft10_068%f[^%w_]') and not modules:find('%f[%w_]minecraft10_069%f[^%w_]') and not modules:find('%f[%w_]minecraft10_070%f[^%w_]') and not modules:find('%f[%w_]minecraft10_071%f[^%w_]') and not modules:find('%f[%w_]minecraft10_072%f[^%w_]'),'E_OLD_MODULE')
   if modules:find('%f[%w_]'..MODULE..'%f[^%w_]') then
    local resident=state()
    if resident.ready then ready();return end
    -- A watchface VM can be recreated while the native module survives.
    -- Resume this exact healthy build/sequence; never insmod or register it twice.
    -- The cold-start interruption gate below still prevents repeated loading.
    assert(resident.phase==1 or resident.phase==2 or (resident.phase>=20 and resident.phase<=25),'E_REGISTER_STATE')
    phase='WAIT';polls=0;return
   end
   local prior=previousAttempt();interruptedStage=prior:match('^([A-Z][A-Z0-9]*)');assert(prior=='' or prior=='READY '..EXPECT_FW..' '..EXPECT_BUILD,'E_INTERRUPTED');interruptedStage=nil
   destination=DEST;beginCopy('game.elf',destination,EXPECT_BYTES)
  elseif phase=='COPY' then
   local b=source:read(math.min(32768,total-at));assert(type(b)=='string' and #b>0 and #b<=total-at,'E_RESOURCE_READ');assert(target:write(b),'E_COPY_WRITE');at=at+#b
   label:set{text='Minecraft · 10 Pro\n复制原生程序 '..math.floor(at*100/total)..'%\n固件 '..EXPECT_FW}
   if at==total then assert(target:flush(),'E_COPY_FLUSH');close(target);target=nil;assert(source:seek('set',0),'E_RESOURCE_SEEK');verify=assert(io.open(destination,'rb'),'E_VERIFY_OPEN');phase='VERIFY';at=0 end
  elseif phase=='VERIFY' then
   local amount=math.min(32768,total-at);local a=source:read(amount);local b=verify:read(amount);assert(a and b and #a==amount and a==b,'E_VERIFY_BYTES');at=at+amount
   if at==total then assert(not verify:read(1),'E_VERIFY_TRAILING');close(source);close(verify);source,verify=nil,nil
    if destination==DEST then destination='/data/minecraft10-grass.png';beginCopy('grass.png',destination,37702)
    else phase='LOAD' end
   end
  elseif phase=='LOAD' then
   label:set{text='Minecraft · 10 Pro\n正在加载原生程序…\n分段核对固件\n请稍候'}
   stage('L0 '..EXPECT_FW..' '..EXPECT_BUILD)
   execute('insmod '..DEST..' '..MODULE,'/tmp/minecraft10-insmod.txt');stage('H0 '..EXPECT_FW..' '..EXPECT_BUILD);phase='WAIT';polls=0
  elseif phase=='WAIT' then
   local s=state();if s.ready then ready();return end
   assert(s.hashed<=AP_BYTES,'E_REGISTER_STATE')
   if s.hashed<AP_BYTES then
    local percent=math.floor(s.hashed*100/AP_BYTES)
    stage('H'..percent..' '..EXPECT_FW..' '..EXPECT_BUILD)
    label:set{text='Minecraft · 10 Pro\n核对固件 '..percent..'%\n0.7.3 · 请稍候'}
    step(s.sequence+1)
   elseif s.phase==2 then
    stage('Q0 '..EXPECT_FW..' '..EXPECT_BUILD)
    label:set{text='Minecraft · 10 Pro\n固件核对通过\n准备原生注册…'}
    step(s.sequence+1)
   else
    label:set{text='Minecraft · 10 Pro\n原生注册 R'..math.max(0,s.phase-20)..'\n0.7.3 · 请稍候'}
   end
   polls=polls+1;assert(polls<math.ceil(AP_BYTES/8192)+600,'E_REGISTER_TIMEOUT')
  end
 end)
 if not worked then fail(err)end
end
root:onevent(lvgl.EVENT.CLICKED,function()
 if not destroyed and finished and retry then
  local ok,err=pcall(attempt,'');if not ok then fail(err);return end
  lastStage,interruptedStage,nativeStage='',nil,nil;finished,retry=false,false;phase='BEGIN';timer=lvgl.Timer{period=12,repeat_count=-1,cb=tick}
 end
end)
root:onevent(lvgl.EVENT.DELETE,function()if destroyed then return end;destroyed=true;clockActive=false;closeAll();if timer then pcall(timer.delete,timer);timer=nil end end)
phase='BEGIN';timer=lvgl.Timer{period=12,repeat_count=-1,cb=tick}
