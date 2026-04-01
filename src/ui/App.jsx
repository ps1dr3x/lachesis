'use strict'

import React, { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Container, Tab } from 'semantic-ui-react'
import Header from './components/Header'
import DataTable from './components/DataTable'
import Footer from './components/Footer'
import 'semantic-ui-css/semantic.min.css'
import './style/app.scss'

const panes = [
  {
    menuItem: 'Records',
    render: () => <Tab.Pane attached={false}><DataTable /></Tab.Pane>
  },
  {
    menuItem: 'Map',
    render: () => <Tab.Pane attached={false}>TODO</Tab.Pane>
  }
]

function App () {
  const [active, setActive] = useState('Records')

  function handleTabChange (e, el) {
    setActive(panes[el.activeIndex].menuItem)
  }

  return (
    <div className='app'>
      <Container>
        <Header title='Lachesis UI' />
        <Tab
          menu={{ secondary: true, pointing: true }}
          panes={panes}
          onTabChange={handleTabChange}
          className={active === 'Records' ? 'nopadding' : ''}
        />
        <Footer version='v0.4.0' />
      </Container>
    </div>
  )
}

createRoot(document.querySelector('#root')).render(<App />)
