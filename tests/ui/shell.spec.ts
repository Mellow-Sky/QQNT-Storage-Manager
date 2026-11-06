import { test, expect } from '@playwright/test';

test('compact browser shell keeps filters and settings out of the file list', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('http://127.0.0.1:1420');
  await expect(page.getByRole('heading', { name: '文件', exact: true })).toBeVisible();
  await expect(page.getByText('浏览器预览 · 本地文件请使用桌面程序')).toBeVisible();
  await expect(page.getByRole('button', { name: '选择目录', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: '删除', exact: true })).toBeDisabled();
  await expect(page.getByText('最小 MiB', { exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '筛选', exact: true }).click();
  await expect(page.getByText('最小 MiB', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '筛选', exact: true }).click();
  for (const [tab, heading] of [['图片', '图片'], ['视频', '视频'], ['大文件', '大文件'], ['全部', '文件']]) {
    await page.getByRole('navigation').getByRole('button', { name: tab, exact: true }).click();
    await expect(page.getByRole('heading', { name: heading, exact: true })).toBeVisible();
  }
  for (const text of ['提交清理审核', '生成 Dry Run 报告', '清理中心', '工作空间']) {
    await expect(page.getByText(text, { exact: true })).toHaveCount(0);
  }
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await expect(page.getByRole('heading', { name: '设置' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await page.getByRole('button', { name: '切换明暗主题' }).click();
  await expect(page.locator('html')).toHaveClass('dark');
  await page.reload();
  await expect(page.locator('html')).toHaveClass('dark');
  await page.getByRole('button', { name: '切换明暗主题' }).click();
  expect(errors).toEqual([]);
  await page.screenshot({ path: 'artifacts/browser-simple.png', fullPage: true });
});

async function mockIndex(page: import('@playwright/test').Page, images: number, database = false) {
  await page.addInitScript(({ images, database }) => {
    const now = 1793970000;
    const root = 'C:\\MOCK-DATA';
    const state = window as unknown as { isTauri: boolean; __calls: string[]; __lastDelete: Record<string, unknown>; __TAURI_INTERNALS__: { invoke: (name: string, args?: Record<string, unknown>) => Promise<unknown> } };
    let files = Array.from({length:images},(_,n)=>({ relativePath:`Pic/MOCK-${String(n).padStart(6,'0')}.png`,directory:'Pic',size:134,kind:'Image',risk:'ReviewRequired',userProtected:false,evidence:[],modified:now,created:now }));
    if(database) files.push({relativePath:'nt_db/MOCK-chat.db',directory:'nt_db',size:18,kind:'Other',risk:'Protected',userProtected:false,evidence:['database'],modified:now,created:now});
    let scanned=false;
    const session=()=>({id:'MOCK-session',root,started:now,finished:now,status:'completed',detection:{recognized:true},summary:{progress:{files:files.length,bytes:files.reduce((sum,file)=>sum+file.size,0)},byKind:{Image:{bytes:files.filter(file=>file.kind==='Image').reduce((sum,file)=>sum+file.size,0)},Other:{bytes:files.filter(file=>file.kind==='Other').reduce((sum,file)=>sum+file.size,0)}}}});
    const matching=(query: Record<string,unknown>)=>files.filter(file=>(!query.kind || file.kind===query.kind) && (!query.search || file.relativePath.includes(String(query.search))));
    state.isTauri=true;state.__calls=[];
    state.__TAURI_INTERNALS__={invoke:async(name,args={})=>{
      state.__calls.push(name);
      if(name==='scan_status') return {state:scanned?'completed':'idle',sessionId:scanned?'MOCK-session':null,progress:session().summary.progress};
      if(name==='list_sessions') return [session()];
      if(name==='get_session') return session();
      if(name==='choose_directory') return [{id:'MOCK-grant',path:root,detection:{recognized:true}}];
      if(name==='start_scan') {scanned=true;return 'MOCK-session';}
      if(name==='disk_status') return {availableBytes:10000000000,lowDisk:false,thresholdBytes:1000000000};
      if(name==='list_files') {const query=args.query as Record<string,unknown>;const rows=matching(query);const offset=Number(query.offset??0);return {total:rows.length,protectedCount:rows.filter(file=>file.userProtected).length,files:rows.slice(offset,offset+Number(query.limit??200))};}
      if(name==='delete_files' || name==='delete_matching') {
        state.__lastDelete={name,...args};
        const selected=name==='delete_files'?files.filter(file=>(args.paths as string[]).includes(file.relativePath)):matching(args.query as Record<string,unknown>).filter(file=>!(args.excludedPaths as string[]).includes(file.relativePath));
        const deleted=selected.filter(file=>file.risk==='ReviewRequired');
        files=files.filter(file=>!deleted.includes(file));
        return {deleted:deleted.slice(0,200).map(file=>file.relativePath),retained:[],failures:[],bytes:deleted.reduce((sum,file)=>sum+file.size,0),total:selected.length,processed:selected.length,deletedCount:deleted.length,retainedCount:0,failedCount:0,remaining:0,cancelled:false,stoppedReason:null,session:session()};
      }
      throw new Error(`Unexpected command: ${name}`);
    }};
  }, { images, database });
  await page.goto('http://127.0.0.1:1420');
  await page.getByRole('button',{name:'选择目录',exact:true}).click();
  await expect(page.getByRole('button',{name:'重新扫描',exact:true})).toBeEnabled();
}

test('delete updates rows and totals with no new scan or confirmation workflow', async ({page})=>{
  await mockIndex(page,1,true);
  await page.getByRole('checkbox',{name:'选择 Pic/MOCK-000000.png',exact:true}).check();
  await page.getByRole('button',{name:'删除',exact:true}).click();
  await expect(page.getByText('已删除 1 项 · 134 B')).toBeVisible();
  await expect(page.getByRole('checkbox',{name:'选择 Pic/MOCK-000000.png',exact:true})).toHaveCount(0);
  await expect(page.getByRole('checkbox',{name:'选择 nt_db/MOCK-chat.db',exact:true})).toBeVisible();
  await expect(page.getByTestId('total-size')).toHaveText('18 B');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  const calls=await page.evaluate(()=>(window as unknown as {__calls:string[]}).__calls);
  expect(calls.filter(name=>name==='delete_files')).toHaveLength(1);
  expect(calls.filter(name=>name==='start_scan')).toHaveLength(1);
  expect(calls.some(name=>['plan_cleanup','execute_cleanup','enable_cleanup','quarantine_action'].includes(name))).toBe(false);
});

test('manual selection survives pagination',async({page})=>{
  await mockIndex(page,450);
  await page.getByRole('checkbox',{name:'选择 Pic/MOCK-000000.png',exact:true}).check();
  await page.getByRole('button',{name:'下一页',exact:true}).click();
  await page.getByRole('checkbox',{name:'选择 Pic/MOCK-000200.png',exact:true}).check();
  await expect(page.getByText('已选 2 项',{exact:true})).toBeVisible();
  await page.getByRole('button',{name:'上一页',exact:true}).click();
  await expect(page.getByRole('checkbox',{name:'选择 Pic/MOCK-000000.png',exact:true})).toBeChecked();
  await page.getByRole('button',{name:'删除',exact:true}).click();
  await expect(page.getByText('已删除 2 项 · 268 B')).toBeVisible();
  const request=await page.evaluate(()=>(window as unknown as {__lastDelete:{paths:string[]}}).__lastDelete);
  expect(request.paths.sort()).toEqual(['Pic/MOCK-000000.png','Pic/MOCK-000200.png']);
});

test('select all includes every matching page and preserves exclusions across pages',async({page})=>{
  await mockIndex(page,450);
  await page.getByRole('checkbox',{name:'全选筛选结果',exact:true}).check();
  await expect(page.getByText('已选 450 项',{exact:true})).toBeVisible();
  await page.getByRole('button',{name:'下一页',exact:true}).click();
  await page.getByRole('checkbox',{name:'选择 Pic/MOCK-000200.png',exact:true}).uncheck();
  await page.getByRole('button',{name:'下一页',exact:true}).click();
  await page.getByRole('checkbox',{name:'选择 Pic/MOCK-000400.png',exact:true}).uncheck();
  await expect(page.getByText('已选 448 项',{exact:true})).toBeVisible();
  await page.getByRole('button',{name:'删除',exact:true}).click();
  await expect(page.getByText('已删除 448 项',{exact:false})).toBeVisible();
  await expect(page.getByTestId('total-size')).toHaveText('268 B');
  await expect(page.getByRole('checkbox',{name:'选择 Pic/MOCK-000200.png',exact:true})).toBeVisible();
  await expect(page.getByRole('checkbox',{name:'选择 Pic/MOCK-000400.png',exact:true})).toBeVisible();
  const state=await page.evaluate(()=>({calls:(window as unknown as {__calls:string[]}).__calls,request:(window as unknown as {__lastDelete:{excludedPaths:string[]}}).__lastDelete}));
  expect(state.request.excludedPaths.sort()).toEqual(['Pic/MOCK-000200.png','Pic/MOCK-000400.png']);
  expect(state.calls.filter(name=>name==='delete_matching')).toHaveLength(1);
  expect(state.calls.filter(name=>name==='start_scan')).toHaveLength(1);
});
